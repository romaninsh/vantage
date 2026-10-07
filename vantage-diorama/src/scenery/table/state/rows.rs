//! Reading and refreshing individual rows and the loaded window.

use std::ops::Range;
use std::sync::Arc;

use crate::dio::DioInner;
use crate::scenery::enriched_record::{EnrichedRecord, RowStatus};

use super::super::ViewportRequest;
use super::TableSceneryState;

impl TableSceneryState {
    /// The cache this view reads rows from: the Dio's own, or the sugared
    /// wrapper over it. Writes — and read-backs that feed a write — always
    /// use the Dio's cache, so sugar never reaches it.
    pub(crate) fn reader(
        &self,
        dio_inner: &DioInner,
    ) -> Arc<dyn crate::lens::cache_backend::CacheTable> {
        self.read_cache
            .clone()
            .unwrap_or_else(|| dio_inner.cache.clone())
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
        let Some(rec) = self.reader(&dio_inner).get_value(id).await? else {
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
        let Ok(Some(rec)) = self.reader(&dio_inner).get_value(id).await else {
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
    ///
    /// Reads the decision [`paged`](Self::paged) recorded at open, for the
    /// reason given there. Asking the lens instead makes every scenery under a
    /// shared lens claim to be paged, and an eager one then answers a landed
    /// dataset by re-fetching a viewport it never fetches through — so the rows
    /// its `on_start` just wrote to the cache never reach the visible map, and
    /// the grid stays empty until something reopens it over the warm cache.
    pub(crate) fn is_chunk_loaded(&self) -> bool {
        !self.two_pass && self.paged
    }

    /// Re-fetch the loaded rows in place so a refresh updates them without
    /// blanking: `force_load` overwrites each slot as the fresh rows land, and a
    /// failed refetch leaves the existing rows untouched (the loader never clears
    /// on error). No-op until a viewport has been set.
    ///
    /// Re-fetches the whole **contiguous loaded block** that contains the
    /// viewport, not just the viewport itself. The master serves rows by absolute
    /// offset; if its order shifted since the last fetch — e.g. a `-last_updated`
    /// order the live source keeps bumping — re-fetching only the viewport leaves
    /// a row that migrated *into* it still sitting at its old slot, i.e. a
    /// duplicate (and another row silently dropped). Overwriting the entire
    /// contiguous block keeps every loaded slot consistent with the master's
    /// current order, so a reorder reshuffles cleanly instead of scrambling.
    ///
    /// `priority` is the caller's, because a forced re-fetch of a cached block
    /// is not one thing: a poll, or the re-drive after a two-pass list page, is
    /// nobody waiting (`Background`); a search, a filter change or a re-order
    /// is a user who typed something and is watching for the result
    /// (`Essential`).
    pub(crate) fn refresh_loaded_viewport(&self, priority: vantage_core::Priority) {
        let Some(viewport) = self.last_viewport.read().unwrap().clone() else {
            return;
        };
        // Two-pass sceneries seed their whole index into the sparse map, so
        // the contiguous-block expansion below would cover EVERYTHING — and
        // a two-pass viewport drives per-row detail fetches, not a cheap
        // block overwrite. Re-issue exactly what the consumer last declared
        // visible; the reorder concern doesn't apply (order lives in the
        // index, rebuilt by `refresh_index`).
        let range = if self.two_pass {
            viewport
        } else {
            let rows = self.rows.read().unwrap();
            let mut start = viewport.start;
            while start > 0 && rows.contains_key(&(start - 1)) {
                start -= 1;
            }
            let mut end = viewport.end;
            while rows.contains_key(&end) {
                end += 1;
            }
            start..end
        };
        super::super::loader::enqueue_viewport(
            self,
            ViewportRequest {
                range,
                force_load: true,
                priority,
            },
        );
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
