use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use ciborium::Value as CborValue;
use tokio::sync::{Notify, mpsc, watch};
use vantage_types::Record;
use vantage_vista::VistaCapabilities;

use crate::dio::{DioInner, Generation};
use crate::lens::SceneryChunkTarget;
use crate::scenery::enriched_record::EnrichedRecord;

use super::helpers::{cbor_cmp, matches_conditions, matches_search};
use super::{SortDir, ViewportRequest};

/// Internal state shared by the public scenery handle, the reactor
/// task, and the viewport-debounce task.
pub(crate) struct TableSceneryState {
    /// Weak so the Scenery doesn't pin the Dio alive — the spawned
    /// tasks exit when the last user-held Dio drops.
    pub(crate) dio_weak: std::sync::Weak<DioInner>,

    pub(crate) conditions: RwLock<Vec<(String, CborValue)>>,
    pub(crate) sort: RwLock<Option<(String, SortDir)>>,
    pub(crate) search: RwLock<Option<String>>,

    pub(crate) rows: RwLock<BTreeMap<usize, Arc<EnrichedRecord>>>,
    pub(crate) id_to_idx: RwLock<HashMap<String, usize>>,
    pub(crate) total: RwLock<Option<usize>>,

    pub(crate) page_size: usize,

    pub(crate) generation: AtomicU64,
    pub(crate) generation_tx: watch::Sender<Generation>,

    pub(crate) reload_notify: Arc<Notify>,
    pub(crate) viewport_tx: mpsc::UnboundedSender<ViewportRequest>,

    /// Mirrors the live depth of `viewport_tx`. Bumped on every send,
    /// decremented every time the loader pops a message. Surfaces the
    /// backlog when chunk fetches can't keep up with scroll bursts.
    pub(crate) viewport_queue_depth: AtomicUsize,

    /// True while a chunk load is currently dispatched — prevents
    /// `request_load_more` from queueing the same range twice in a row.
    pub(crate) load_in_flight: Mutex<Option<Range<usize>>>,

    /// Snapshot of the master Vista's capability flags taken at open
    /// time. Sceneries hand this back through
    /// `TableScenery::master_capabilities` so UI delegates can route
    /// page requests through the right primitive (`set_viewport` for
    /// `can_fetch_page`, `request_load_more` for `can_fetch_next`).
    pub(crate) master_capabilities: VistaCapabilities,
}

impl TableSceneryState {
    pub(crate) fn bump_generation(&self) {
        let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.generation_tx.send_replace(Generation(next));
    }

    pub(crate) fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Replace the sparse map from a freshly-listed cache snapshot.
    /// Applies conditions/search/sort in memory (v1-compat path).
    pub(crate) async fn reseed_from_cache(&self) -> vantage_core::Result<()> {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return Ok(());
        };
        let all = dio_inner.cache.list_values().await?;

        let conditions = self.conditions.read().unwrap().clone();
        let search = self.search.read().unwrap().clone();
        let sort = self.sort.read().unwrap().clone();

        let mut filtered: Vec<(String, Record<CborValue>)> = all
            .into_iter()
            .filter(|(_, rec)| matches_conditions(rec, &conditions))
            .filter(|(_, rec)| matches_search(rec, search.as_deref()))
            .collect();

        if let Some((col, dir)) = sort {
            filtered.sort_by(|(_, a), (_, b)| {
                let ord = cbor_cmp(a.get(&col), b.get(&col));
                match dir {
                    SortDir::Asc => ord,
                    SortDir::Desc => ord.reverse(),
                }
            });
        }

        let mut rows = BTreeMap::new();
        let mut id_to_idx = HashMap::new();
        for (idx, (id, rec)) in filtered.into_iter().enumerate() {
            rows.insert(idx, Arc::new(EnrichedRecord::fresh(rec)));
            id_to_idx.insert(id, idx);
        }
        *self.rows.write().unwrap() = rows;
        *self.id_to_idx.write().unwrap() = id_to_idx;
        Ok(())
    }

    /// React to `DioEvent::RecordChanged { id }`: if the id is in our
    /// sparse map, re-read it from cache and update the slot in place.
    /// Bumps generation.
    pub(crate) async fn update_by_id(&self, id: &str) -> vantage_core::Result<()> {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return Ok(());
        };
        let idx = match self.id_to_idx.read().unwrap().get(id).copied() {
            Some(i) => i,
            None => return Ok(()),
        };
        let Some(rec) = dio_inner.cache.get_value(id).await? else {
            return Ok(());
        };
        self.rows
            .write()
            .unwrap()
            .insert(idx, Arc::new(EnrichedRecord::fresh(rec)));
        self.bump_generation();
        Ok(())
    }

    /// True if every index in `range` is loaded.
    pub(crate) fn range_fully_cached(&self, range: &Range<usize>) -> bool {
        let rows = self.rows.read().unwrap();
        range.clone().all(|i| rows.contains_key(&i))
    }

    /// Largest cached index, +1 — the natural start for the next
    /// `request_load_more` chunk.
    pub(crate) fn next_load_more_start(&self) -> usize {
        self.rows
            .read()
            .unwrap()
            .keys()
            .next_back()
            .copied()
            .map(|n| n + 1)
            .unwrap_or(0)
    }
}

impl SceneryChunkTarget for TableSceneryState {
    fn write_chunk_row(&self, idx: usize, id: String, record: Record<CborValue>) {
        let enriched = Arc::new(EnrichedRecord::fresh(record));
        self.rows.write().unwrap().insert(idx, enriched);
        self.id_to_idx.write().unwrap().insert(id, idx);
    }
}
