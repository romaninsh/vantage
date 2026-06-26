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
use crate::scenery::enriched_record::{EnrichedRecord, RowStatus};

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

    /// The most recent viewport range handed to the loader. A refresh on a
    /// chunk-loaded scenery re-fetches exactly this range in place (see
    /// [`refresh_loaded_viewport`](Self::refresh_loaded_viewport)). `None`
    /// until the first viewport is set.
    pub(crate) last_viewport: RwLock<Option<Range<usize>>>,

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

    /// Set by [`write_chunk_row`](Self::write_chunk_row) whenever a chunk load
    /// actually changes a row's visible content (new slot, status change, or a
    /// different record). The loader reads and clears it after the load and
    /// bumps the generation only when it is set — so a refresh that re-fetches
    /// byte-identical rows does not signal a repaint.
    pub(crate) load_dirty: std::sync::atomic::AtomicBool,

    /// Count of rows the in-flight chunk load *received* (every push, including
    /// those `write_chunk_row` skips as unchanged). A short page — fewer rows
    /// than the requested window — means the end of the set, so the loader
    /// derives the grand `total` from it (no separate count request).
    pub(crate) load_push_count: AtomicUsize,

    /// Snapshot of the master Vista's capability flags taken at open
    /// time. Sceneries hand this back through
    /// `TableScenery::master_capabilities` so UI delegates can route
    /// page requests through the right primitive (`set_viewport` for
    /// `can_fetch_page`, `request_load_more` for `can_fetch_next`).
    pub(crate) master_capabilities: VistaCapabilities,

    // ---- two-pass loading -------------------------------------------------
    //
    // Populated only when the Lens registers an `on_load_detail` callback.
    // `two_pass == false` leaves every field below inert and the scenery on
    // the legacy single-pass path.
    /// Whether this scenery drives two-pass (list + detail) loading.
    pub(crate) two_pass: bool,
    /// Dropdown / autocomplete projection: serve the cheap list columns and
    /// **skip the detail pass** even on a two-pass table. The list pass still
    /// runs (rows carry id + title columns); per-row hydration never fires.
    pub(crate) titles_only: bool,
    /// The shared per-query ordered index for this scenery's conditions/sort,
    /// keyed by [`Vista::index_key`](vantage_vista::Vista::index_key). `None` in single-pass mode.
    /// Swappable: a `set_sort` / `set_search` re-points it at the index for the
    /// new variant (see [`resort`](super::two_pass::resort)).
    pub(crate) index: RwLock<Option<Arc<crate::dio::query_index::QueryIndex>>>,
    /// This scenery's key in the Dio's dedup registry, captured at open. Cleared
    /// (and the registry entry removed) the first time the handle mutates its own
    /// query in place — a bespoke, resorted scenery is no longer the shareable
    /// canonical one, so a later open under the old key must not get it back.
    pub(crate) registry_key: Mutex<Option<String>>,
    /// Ids whose detail fetch is currently dispatched — guards against
    /// re-hydrating the same row while a fetch is in flight.
    pub(crate) detail_in_flight: Mutex<std::collections::HashSet<String>>,
    /// True while a list-page fetch is dispatched, so overlapping
    /// `request_load_more` calls don't double-page.
    pub(crate) list_in_flight: Mutex<bool>,
}

impl TableSceneryState {
    pub(crate) fn bump_generation(&self) {
        let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.generation_tx.send_replace(Generation(next));
    }

    pub(crate) fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Clear the per-load trackers (dirty flag + push count) before dispatching
    /// a load, so they reflect only the rows written by that load.
    pub(crate) fn reset_load_dirty(&self) {
        self.load_dirty.store(false, Ordering::SeqCst);
        self.load_push_count.store(0, Ordering::SeqCst);
    }

    /// Read and clear the chunk-load dirty flag. `true` means the load changed
    /// at least one row's content (so a generation bump is warranted).
    pub(crate) fn take_load_dirty(&self) -> bool {
        self.load_dirty.swap(false, Ordering::SeqCst)
    }

    /// Rows the just-finished chunk load received (reads the `load_push_count` field).
    pub(crate) fn load_push_count(&self) -> usize {
        self.load_push_count.load(Ordering::SeqCst)
    }

    /// Overwrite the cached grand total. Returns `true` if it changed (so the
    /// loader can bump the generation for `row_count` consumers).
    pub(crate) fn set_total(&self, total: Option<usize>) -> bool {
        let mut guard = self.total.write().unwrap();
        let changed = *guard != total;
        *guard = total;
        changed
    }

    /// Current two-pass index (cloned `Arc`), or `None` in single-pass mode.
    pub(crate) fn index(&self) -> Option<Arc<crate::dio::query_index::QueryIndex>> {
        self.index.read().unwrap().clone()
    }

    /// Re-point the two-pass index at a different query variant's ordered index.
    pub(crate) fn set_index(&self, index: Option<Arc<crate::dio::query_index::QueryIndex>>) {
        *self.index.write().unwrap() = index;
    }

    /// Drop this scenery's dedup-registry entry the first time it mutates its
    /// own query (sort/search) in place. Idempotent: the key is taken once.
    pub(crate) fn deregister(&self) {
        let Some(key) = self.registry_key.lock().unwrap().take() else {
            return;
        };
        if let Some(dio) = self.dio_weak.upgrade() {
            dio.table_sceneries.lock().unwrap().remove(&key);
        }
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

    /// Stamp the slot for `id` with `status`, re-reading its current cache
    /// value (the optimistic-write affordance — `PendingWrite` while a write is
    /// in flight, `WriteFailed` after a rollback). No-op if the row isn't in
    /// this scenery's window. Bumps generation so bound widgets repaint.
    pub(crate) async fn mark_row(&self, id: &str, status: RowStatus) {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return;
        };
        let Some(idx) = self.id_to_idx.read().unwrap().get(id).copied() else {
            return;
        };
        let Ok(Some(rec)) = dio_inner.cache.get_value(id).await else {
            return;
        };
        let enriched = EnrichedRecord {
            record: rec,
            status,
            dirty_fields: None,
            fetched_at: Some(std::time::SystemTime::now()),
        };
        self.rows.write().unwrap().insert(idx, Arc::new(enriched));
        self.bump_generation();
    }

    /// True if every index in `range` is loaded.
    pub(crate) fn range_fully_cached(&self, range: &Range<usize>) -> bool {
        let rows = self.rows.read().unwrap();
        range.clone().all(|i| rows.contains_key(&i))
    }

    /// True for a single-pass, chunk-loaded scenery (paged/lazy via
    /// `on_load_chunk`). This is the variant whose refresh re-fetches the
    /// visible window in place instead of reseeding from cache — reseeding
    /// would only re-show whatever happens to be cached (and shows nothing
    /// if the cache was just cleared).
    pub(crate) fn is_chunk_loaded(&self) -> bool {
        if self.two_pass {
            return false;
        }
        self.dio_weak
            .upgrade()
            .map(|d| d.lens.callbacks.on_load_chunk.is_some())
            .unwrap_or(false)
    }

    /// Re-fetch the last viewport in place so a refresh updates the visible
    /// rows without blanking them: `force_load` overwrites each slot as the
    /// fresh rows land, and a failed refetch leaves the existing rows
    /// untouched (the loader never clears on error). No-op until a viewport
    /// has been set.
    pub(crate) fn refresh_loaded_viewport(&self) {
        let range = self.last_viewport.read().unwrap().clone();
        if let Some(range) = range {
            super::loader::enqueue_viewport(
                self,
                ViewportRequest {
                    range,
                    force_load: true,
                },
            );
        }
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
        // Count every received row (before the skip below), so the loader can
        // tell a short page (end of set) from a full one.
        self.load_push_count.fetch_add(1, Ordering::SeqCst);
        // Skip the write entirely when this slot already holds the same fresh
        // record: a refresh that re-fetches identical data must not look like a
        // change. Only a new/!Fresh slot or a different record is "dirty", and
        // only a dirty load bumps the generation (see `loader::fire_chunk_load`).
        {
            let rows = self.rows.read().unwrap();
            if let Some(existing) = rows.get(&idx)
                && existing.status == RowStatus::Fresh
                && existing.record == record
            {
                return;
            }
        }
        let enriched = Arc::new(EnrichedRecord::fresh(record));
        self.rows.write().unwrap().insert(idx, enriched);
        self.id_to_idx.write().unwrap().insert(id, idx);
        self.load_dirty.store(true, Ordering::SeqCst);
    }
}
