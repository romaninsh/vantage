//! `RecordScenery`: one row by id, following its changes.
//!
//! Opened through [`Dio::record_scenery`](crate::Dio::record_scenery), it
//! shows the cached row and a [`RecordStatus`], and bumps a watch channel when
//! the row changes. A row outside the Dio's residual is `NotFound`. The status
//! table is in `README_ui.md`, "RecordScenery". An in-flight write shows on the
//! row's [`EnrichedRecord`], not as a status; dirty fields are the
//! [`Servo`](crate::servo::Servo)'s.

pub(crate) mod builder;
mod handle;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock, Weak};

use ciborium::Value as CborValue;
use tokio::sync::watch;
use vantage_core::Result;
use vantage_types::Record;

use super::enriched_record::EnrichedRecord;
use crate::dio::augment_scheduler::AugmentTicket;
use crate::dio::{DetailState, DioInner, Generation};
use crate::lens::CacheStatus;
use crate::lens::cache_backend::CacheTable;

pub use builder::RecordSceneryBuilder;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecordStatus {
    /// A [`load_on_miss`](RecordSceneryBuilder::load_on_miss) fetch is in
    /// flight.
    Loading,
    /// The row is shown, but its augmented values haven't been fetched yet.
    Incomplete,
    Fresh,
    NotFound,
    Error(String),
}

/// Reactive view onto a single record by id within a Dio.
pub trait RecordScenery: Send + Sync {
    fn record(&self) -> Option<Arc<EnrichedRecord>>;
    fn status(&self) -> RecordStatus;

    /// Re-read the Dio's set from its source; the row reloads when it lands.
    fn refresh_source(&self);
    fn subscribe(&self) -> watch::Receiver<Generation>;
}

pub(crate) struct RecordSceneryState {
    /// Live-instance census (see [`crate::stats`]).
    pub(crate) _tally: crate::stats::Tally,
    pub(crate) dio_weak: Weak<DioInner>,
    pub(crate) id: String,
    /// A sugared wrapper this view reads its row through, when opened with
    /// [`RecordSceneryBuilder::sugar`].
    pub(crate) read_cache: Option<Arc<dyn CacheTable>>,

    pub(crate) record: RwLock<Option<Arc<EnrichedRecord>>>,
    pub(crate) status: RwLock<RecordStatus>,
    /// Set while a `load_on_miss` fetch is in flight: a reload that finds
    /// nothing meanwhile keeps `Loading`.
    pub(crate) fetching: AtomicBool,
    /// This view's queue into the augment scheduler, made on the first
    /// `Incomplete` read.
    pub(crate) hydration: OnceLock<AugmentTicket>,

    pub(crate) generation: AtomicU64,
    pub(crate) generation_tx: watch::Sender<Generation>,
}

impl RecordSceneryState {
    /// The cache this view reads from: the Dio's own, or the sugared wrapper.
    fn reader(&self, dio_inner: &DioInner) -> Arc<dyn CacheTable> {
        self.read_cache
            .clone()
            .unwrap_or_else(|| dio_inner.cache.clone())
    }

    /// The row the Dio admits, and whether it still waits for its detail
    /// pass (it is queued for one when it does). `None` when the cache
    /// doesn't hold it, or holds it outside the residual.
    pub(crate) async fn read(
        &self,
        dio: &Arc<DioInner>,
    ) -> Result<Option<(Record<CborValue>, bool)>> {
        let reader = self.reader(dio);
        let Some(row) = dio.read_admitted(reader.as_ref(), &self.id).await? else {
            return Ok(None);
        };
        let incomplete = self.awaits_detail(dio).await?;
        Ok(Some((row, incomplete)))
    }

    /// Whether the cached row waits for the detail pass; queues it, one
    /// flight per row through the scheduler, when it does. A row the pass
    /// settled empty, or failed on, waits no longer.
    async fn awaits_detail(&self, dio: &Arc<DioInner>) -> Result<bool> {
        let id = self.id.as_str();
        if !dio.has_dio_augment()
            || matches!(
                dio.detail_state(id),
                Some(DetailState::Empty | DetailState::Failed)
            )
        {
            return Ok(false);
        }
        let Some((row, status)) = dio.cache.get_value_with_status(id).await? else {
            return Ok(false);
        };
        if status != CacheStatus::Incomplete && !dio.awaits_hydration(id, &row, status) {
            return Ok(false);
        }
        dio.ensure_augment_workers();
        self.hydration
            .get_or_init(|| dio.augment_scheduler.ticket())
            .enqueue([self.id.clone()]);
        Ok(true)
    }

    /// What `open` starts on, written without a bump. A reload that already
    /// published read the cache later, so it wins; except a `Loading` it
    /// held for a fetch `open` then decided against.
    pub(crate) fn open_on(&self, record: Option<Arc<EnrichedRecord>>, status: RecordStatus) {
        let mut shown = self.record.write().unwrap();
        let mut current = self.status.write().unwrap();
        if self.generation.load(Ordering::SeqCst) == 0 {
            (*shown, *current) = (record, status);
        } else if *current == RecordStatus::Loading && !self.fetching.load(Ordering::SeqCst) {
            (*shown, *current) = (record, status);
            self.bump_generation();
        }
    }

    /// Change what is shown and bump, all under the record lock, so
    /// [`open_on`](Self::open_on) can tell a reload got there first.
    fn publish(&self, change: impl FnOnce(&mut Option<Arc<EnrichedRecord>>, &mut RecordStatus)) {
        let mut shown = self.record.write().unwrap();
        let mut current = self.status.write().unwrap();
        change(&mut shown, &mut current);
        self.bump_generation();
    }

    /// [`publish`](Self::publish) a change computed from a read that began
    /// at generation `seen`, unless a newer write landed since. With
    /// `over_loading`, a `Loading` (a reload's placeholder for the fetch this
    /// change settles) is replaced anyway.
    fn publish_since(
        &self,
        seen: u64,
        over_loading: bool,
        change: impl FnOnce(&mut Option<Arc<EnrichedRecord>>, &mut RecordStatus),
    ) {
        let mut shown = self.record.write().unwrap();
        let mut current = self.status.write().unwrap();
        let unchanged = self.generation.load(Ordering::SeqCst) == seen;
        if unchanged || (over_loading && *current == RecordStatus::Loading) {
            change(&mut shown, &mut current);
            self.bump_generation();
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Re-read the row from cache, update record + status, bump generation.
    /// A row outside the Dio's residual is not found.
    async fn reload(&self) {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return;
        };
        let (record, status) = match self.read(&dio_inner).await {
            Ok(Some((row, incomplete))) => shown(row, incomplete),
            Ok(None) if self.fetching.load(Ordering::SeqCst) => (None, RecordStatus::Loading),
            Ok(None) => (None, RecordStatus::NotFound),
            Err(e) => return self.set_error(e.to_string()),
        };
        self.publish(|shown, current| (*shown, *current) = (record, status));
    }

    /// Keeps the row shown, if any, and reports why it isn't current.
    pub(crate) fn set_error(&self, msg: String) {
        self.publish(|_, current| *current = RecordStatus::Error(msg));
    }

    /// The detail pass for this row failed: the partial row stays shown,
    /// flagged with the failure.
    fn set_detail_failed(&self, error: String) {
        self.publish(|shown, current| {
            if let Some(row) = shown.as_ref() {
                let failed = EnrichedRecord::detail_failed(
                    row.record.clone(),
                    error.clone(),
                    row.fetched_at,
                );
                *shown = Some(Arc::new(failed));
            }
            *current = RecordStatus::Error(error);
        });
    }

    fn bump_generation(&self) {
        let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        // `send_replace` (not `send`) — the stored value must reflect the
        // current generation even when there are momentarily zero
        // receivers. UIs that drop and re-subscribe must see the latest.
        let _ = self.generation_tx.send_replace(Generation(next));
    }
}

/// `row` as shown, `Incomplete` when it waits for its detail pass.
pub(crate) fn shown(
    row: Record<CborValue>,
    incomplete: bool,
) -> (Option<Arc<EnrichedRecord>>, RecordStatus) {
    let (record, status) = if incomplete {
        (EnrichedRecord::incomplete(row), RecordStatus::Incomplete)
    } else {
        (EnrichedRecord::fresh(row), RecordStatus::Fresh)
    };
    (Some(Arc::new(record)), status)
}
