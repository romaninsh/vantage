//! Settled flag, debug state lines, generation counter and per-load trackers.

use std::sync::atomic::Ordering;

use crate::dio::Generation;

use super::TableSceneryState;

impl TableSceneryState {
    /// Mark the first load complete — the view now shows an answer, even if
    /// that answer is "no rows".
    ///
    /// Logged because settling is the moment a grid stops showing its skeleton
    /// and commits to what it has. Settling early with nothing paints "no rows"
    /// over a load still in flight, and from the outside that is indis-
    /// tinguishable from an empty table — the row count alone never says which.
    /// `reason` names the call site. Which of them settles a given scenery is
    /// the whole question when a grid shows "no rows" too early, and it is not
    /// recoverable from the state afterwards — the flag records that someone
    /// settled it, never who.
    pub(crate) fn mark_settled(&self, reason: &'static str) {
        if !self.settled.swap(true, Ordering::SeqCst) {
            tracing::debug!(
                target: "vantage_diorama::cache",
                reason,
                table = self
                    .dio_weak
                    .upgrade()
                    .map(|d| d.cache_table_name.clone())
                    .unwrap_or_default(),
                rows = self.rows.read().unwrap().len(),
                total = ?*self.total.read().unwrap(),
                paged = self.paged,
                two_pass = self.two_pass,
                "scenery settled — the grid now shows this as the answer",
            );
            self.note_state(reason);
            self.publish_view_stats();
        }
    }

    /// Emit a `"state"` debug line when this scenery's [`LoadState`](super::super::LoadState)
    /// has changed since the last call. Checks the tap BEFORE computing the
    /// state — the computation takes the `rows`/`total` locks, a cost the
    /// off path must not pay.
    pub(crate) fn note_state(&self, reason: &'static str) {
        if !self.debug_tap.enabled() {
            return;
        }
        let to = super::super::compute_load_state(self);
        let mut last = self.last_debug_state.lock().unwrap();
        if *last == Some(to) {
            return;
        }
        let from = *last;
        *last = Some(to);
        drop(last);
        crate::debug::tapline!(
            self.debug_tap,
            "scenery",
            "{} → {} ({})",
            from.map(|s| s.as_str()).unwrap_or("opening"),
            to.as_str(),
            reason,
        );
    }

    pub(crate) fn bump_generation(&self) {
        let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.generation_tx.send_replace(Generation(next));
        self.publish_view_stats();
    }

    /// Release the two-pass list single-flight and republish, so the
    /// snapshot stops reporting a fetch in flight.
    pub(crate) fn clear_list_in_flight(&self) {
        *self.list_in_flight.lock().unwrap() = false;
        self.publish_view_stats();
    }

    pub(crate) fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Clear the per-load trackers (dirty flag + push count) before dispatching
    /// a load, so they reflect only the rows written by that load.
    pub(crate) fn reset_load_dirty(&self) {
        self.load_dirty.store(false, Ordering::SeqCst);
        self.load_push_count.store(0, Ordering::SeqCst);
        self.load_total_reported.store(false, Ordering::SeqCst);
    }

    /// Whether the just-finished load stated a grand total, clearing the flag.
    pub(crate) fn take_load_total_reported(&self) -> bool {
        self.load_total_reported.swap(false, Ordering::SeqCst)
    }

    /// Whether any fetch has ever stated a total — see the field docs.
    pub(crate) fn total_ever_stated(&self) -> bool {
        self.total_ever_stated.load(Ordering::SeqCst)
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
}
