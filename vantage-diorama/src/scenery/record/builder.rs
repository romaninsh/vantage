//! [`RecordSceneryBuilder`]: what a record scenery opens on.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet as _;
use vantage_types::Record;

use super::handle::spawn_record_scenery;
use super::{RecordScenery, RecordSceneryState, RecordStatus, shown};
use crate::Sugar;
use crate::dio::{Dio, DioInner, admitted_by_dio};
use crate::ops::ChangeEvent;
use crate::scenery::sugar::SugaredCache;

/// A record scenery for one id, configured before it opens. Start one with
/// [`Dio::record_scenery`](crate::Dio::record_scenery).
#[must_use = "a RecordSceneryBuilder does nothing until `open().await`"]
pub struct RecordSceneryBuilder {
    dio: Arc<DioInner>,
    id: String,
    sugar: Option<Sugar>,
    seed: Option<Record<CborValue>>,
    load_on_miss: bool,
}

impl RecordSceneryBuilder {
    pub(crate) fn new(dio: Arc<DioInner>, id: String) -> Self {
        Self {
            dio,
            id,
            sugar: None,
            seed: None,
            load_on_miss: false,
        }
    }

    /// Read the row with `sugar`'s outputs merged in; nothing reaches the
    /// cache.
    pub fn sugar(mut self, sugar: Sugar) -> Self {
        self.sugar = Some(sugar);
        self
    }

    /// Start from `row`, `Fresh`, without reading the cache: a parent grid
    /// handing its current row to a detail view. With [`sugar`](Self::sugar),
    /// or on an augmented Dio, the cache's copy replaces it in the
    /// background, bringing the sugar columns, or `Incomplete` and the
    /// detail pass; a cache miss keeps the seeded row. Later changes to the
    /// id reload it from the cache as usual.
    pub fn seed(mut self, row: Record<CborValue>) -> Self {
        self.seed = Some(row);
        self
    }

    /// On a cache miss, fetch the row from the master. The scenery reads
    /// `Loading` until the fetch settles. A row the Dio's residual admits is
    /// cached and shown `Fresh`; an absent one, or one outside the residual,
    /// is `NotFound` and caches nothing.
    pub fn load_on_miss(mut self) -> Self {
        self.load_on_miss = true;
        self
    }

    /// Open the scenery. Without [`seed`](Self::seed) the cache is read once
    /// here: a residual naming an augmented value judges the row hydrated,
    /// so the read blocks on its detail pass. A failed read is the error.
    pub async fn open(self) -> Result<Arc<dyn RecordScenery>> {
        let dio = self.dio;
        dio.refresh_if_stale();
        let refines_seed = self.sugar.is_some() || dio.has_dio_augment();
        let reader = self
            .sugar
            .map(|sugar| SugaredCache::wrap(dio.cache.clone(), sugar));
        let may_fetch = self.seed.is_none() && self.load_on_miss;
        let scenery = spawn_record_scenery(&dio, self.id, reader, may_fetch);
        let state = scenery.inner.clone();
        if let Some(row) = self.seed {
            let (record, status) = shown(row, false);
            state.open_on(record, status);
            // The seeded row shows at once; the cache's copy then brings its
            // sugar columns, or its `Incomplete` status and detail pass.
            if refines_seed {
                let seeded = state.clone();
                dio.lens
                    .runtime
                    .spawn(async move { seeded.refine_seed().await });
            }
        } else if let Some((row, incomplete)) = state.read(&dio).await? {
            state.fetching.store(false, Ordering::SeqCst);
            let (record, status) = shown(row, incomplete);
            state.open_on(record, status);
        } else if may_fetch && dio.cache.get_value(&state.id).await?.is_none() {
            dio.lens.runtime.spawn(fetch_on_miss(dio.clone(), state));
        } else {
            state.fetching.store(false, Ordering::SeqCst);
            state.open_on(None, RecordStatus::NotFound);
        }
        Ok(Arc::new(scenery))
    }
}

impl RecordSceneryState {
    /// [`reload`](RecordSceneryState::reload) over a seeded row: a cache hit
    /// replaces it, a miss keeps it. A reload or write that lands during the
    /// read is newer, and is kept.
    async fn refine_seed(&self) {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return;
        };
        let seen = self.generation();
        match self.read(&dio_inner).await {
            Ok(Some((row, incomplete))) => {
                let (record, status) = shown(row, incomplete);
                self.publish_since(seen, false, |shown, current| {
                    (*shown, *current) = (record, status)
                });
            }
            Ok(None) => {}
            Err(e) => self.publish_since(seen, false, |_, current| {
                *current = RecordStatus::Error(e.to_string())
            }),
        }
    }

    /// Settle a `load_on_miss` fetch that began at generation `seen` with
    /// `NotFound`, or with the fetch's error. A reload that found the row
    /// meanwhile is newer and is kept; its `Loading` placeholder is not.
    fn settle_fetch(&self, seen: u64, error: Option<String>) {
        self.publish_since(seen, true, |shown, current| match error {
            Some(msg) => *current = RecordStatus::Error(msg),
            None => (*shown, *current) = (None, RecordStatus::NotFound),
        });
    }
}

/// Fetch the missing row from the master and cache it when the Dio admits
/// it. The `RecordInserted` that publishes makes the scenery reload it.
async fn fetch_on_miss(dio: Arc<DioInner>, state: Arc<RecordSceneryState>) {
    let seen = state.generation();
    let cached = match fetch_admitted(&dio, &state.id).await {
        Ok(Some(row)) => {
            let inserted = ChangeEvent::Inserted {
                id: state.id.clone(),
                new: Some(row),
            };
            (Dio { inner: dio })
                .apply_change(inserted)
                .await
                .map(|()| true)
        }
        Ok(None) => Ok(false),
        Err(e) => Err(e),
    };
    // Cleared only once the row is in the cache, so no reload in between
    // reads the miss as `NotFound`.
    state.fetching.store(false, Ordering::SeqCst);
    match cached {
        Ok(true) => {}
        Ok(false) => state.settle_fetch(seen, None),
        Err(e) => state.settle_fetch(seen, Some(e.to_string())),
    }
}

/// The master's row `id` when this Dio admits it. On an augmented Dio the
/// augmentations run over it in memory first, so the residual judges the
/// augmented row and that row is what gets cached: it is stored `Complete`,
/// and no detail pass would visit it later.
async fn fetch_admitted(dio: &DioInner, id: &str) -> Result<Option<Record<CborValue>>> {
    let Some(row) = dio.master.get_value(id).await? else {
        return Ok(None);
    };
    let row = if dio.has_dio_augment() {
        dio.augment_in_memory(row).await?
    } else {
        row
    };
    Ok(admitted_by_dio(dio, &row).then_some(row))
}
