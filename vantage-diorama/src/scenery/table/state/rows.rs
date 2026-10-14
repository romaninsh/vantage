//! Reading and refreshing individual rows and the loaded window.

use std::ops::Range;
use std::sync::Arc;

use crate::dio::DioInner;
use crate::dio::augment_scheduler::AugmentTicket;
use crate::scenery::enriched_record::{EnrichedRecord, RowStatus};

use super::super::ViewportRequest;
use super::super::view::ViewId;
use super::SharedRows;

impl SharedRows {
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
        let enriched = EnrichedRecord::now(rec, status);
        {
            let mut rows = self.rows.write().unwrap();
            rows.insert(idx, Arc::new(enriched));
            self.forget_idx_to_id();
        }
        self.bump_generation();
    }

    /// True for a single-pass scenery serving its current query from windows
    /// the paged loader reads ([`Serving::Windows`](super::super::plan::Serving)).
    /// This is the variant whose refresh re-fetches the visible window in
    /// place instead of reseeding from cache — reseeding would only re-show
    /// whatever happens to be cached (and shows nothing if the cache was just
    /// cleared).
    ///
    /// Only a view whose [`paged`](Self::paged) decision, recorded at open,
    /// is true can serve windows, for the reason given there. Asking the lens
    /// instead makes every scenery under a shared lens claim to be paged, and
    /// an eager one then answers a landed dataset by re-fetching a viewport it
    /// never fetches through — so the rows its eager start just wrote to the
    /// cache never reach the visible map, and the grid stays empty until
    /// something reopens it over the warm cache.
    pub(crate) fn is_chunk_loaded(&self) -> bool {
        self.serving.read().unwrap().is_windows()
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
    ///
    /// Every view's viewport is re-fetched, each range once: two views on
    /// the same rows may be scrolled to different places.
    pub(crate) fn refresh_loaded_viewport(&self, priority: vantage_core::Priority) {
        let mut requested = Vec::new();
        for (view, viewport) in self.viewports() {
            let range = self.loaded_block(viewport);
            if requested.contains(&range) {
                continue;
            }
            requested.push(range.clone());
            super::super::loader::enqueue_viewport(
                self,
                ViewportRequest {
                    range,
                    force_load: true,
                    priority,
                    view: Some(view),
                },
            );
        }
    }

    /// The range a refresh re-fetches for `viewport`. Two-pass rows seed
    /// their whole index into the sparse map, so the contiguous-block
    /// expansion would cover EVERYTHING — and a two-pass viewport drives
    /// per-row detail fetches, not a cheap block overwrite. Re-issue exactly
    /// what the consumer last declared visible; the reorder concern doesn't
    /// apply (order lives in the index, rebuilt by `refresh_index`).
    fn loaded_block(&self, viewport: Range<usize>) -> Range<usize> {
        if self.two_pass {
            return viewport;
        }
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
    }

    /// Each attached view's viewport, for the views that have one.
    pub(crate) fn viewports(&self) -> Vec<(ViewId, Range<usize>)> {
        let views = self.views.lock().unwrap();
        views
            .iter()
            .filter_map(|(id, hook)| hook.viewport.clone().map(|range| (*id, range)))
            .collect()
    }

    /// Whether any attached view has asked for rows yet.
    pub(crate) fn has_viewport(&self) -> bool {
        let views = self.views.lock().unwrap();
        views.values().any(|hook| hook.viewport.is_some())
    }

    /// Remember `range` as the rows `view` last asked to load.
    pub(crate) fn record_viewport(&self, view: ViewId, range: Range<usize>) {
        if let Some(hook) = self.views.lock().unwrap().get_mut(&view) {
            hook.viewport = Some(range);
        }
    }

    /// The augment ticket a two-pass request from `view` hydrates under: the
    /// view's own, or the sweep's for a request no view made.
    pub(crate) fn ticket_for(&self, view: Option<ViewId>) -> Option<Arc<AugmentTicket>> {
        let Some(view) = view else {
            return self.sweep_ticket.clone();
        };
        let views = self.views.lock().unwrap();
        match views.get(&view) {
            Some(hook) => hook.ticket.clone(),
            None => self.sweep_ticket.clone(),
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
