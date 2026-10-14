//! What the open table views add up to on [`DioInner`]: the census, the
//! demand union, and the augment worker pool the views drive.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::DioInner;
use crate::dio::augment_scheduler;

impl DioInner {
    /// How many table views have a live handle. Reading never changes the
    /// list: closed views are pruned only when a view is registered.
    pub(crate) fn live_table_scenery_count(&self) -> usize {
        let views = self.table_views.lock().unwrap();
        views.iter().filter(|weak| weak.strong_count() > 0).count()
    }

    /// One census line: who is consuming this Dio right now, and what the
    /// process costs. Emitted on every consumer open/close when the tap is
    /// enabled. Returns before any work when it isn't — the off-path must
    /// stay byte-identical to a build without this stream.
    pub(crate) fn emit_census(&self, kind: &'static str, verb: &'static str) {
        let tap = self.tap();
        if !tap.enabled() {
            return;
        }
        let table_sceneries = self.live_table_scenery_count();
        let record_sceneries = self.record_census.load(Ordering::Relaxed);
        let servos = self.servo_census.load(Ordering::Relaxed);
        let peak_rss = crate::debug::peak_rss_bytes();
        crate::debug::tapline!(
            tap,
            "census",
            "{} {} — now {} table, {} record, {} servo · rss {}MB",
            if verb == "opened" { "+1" } else { "-1" },
            kind,
            table_sceneries,
            record_sceneries,
            servos,
            peak_rss / (1024 * 1024),
        );
    }

    /// The union of every live view's demanded columns — the Dio's active
    /// demand. `None` = at least one open view demands everything (or no
    /// view declared a demand): the pre-demand behavior. `Some(union)` =
    /// only these columns are looked at; the augment detail pass runs only
    /// while the union intersects the augment columns. Recomputed on the fly
    /// from the live views, so demand follows views opening and closing with
    /// no extra lifecycle machinery.
    pub(crate) fn demanded_columns(&self) -> Option<HashSet<String>> {
        let mut union = HashSet::new();
        let mut any_live = false;
        for view in self.live_views() {
            any_live = true;
            union.extend(view.demanded_columns()?);
        }
        // No live views at all: stay permissive (a detail pass mid-flight on
        // a closing scenery must not stall on an empty union).
        if !any_live { None } else { Some(union) }
    }

    /// Whether the augment detail pass has work under the current demand:
    /// true when this Dio's augment columns intersect the open sceneries'
    /// demand union (or when either side is un-enumerable).
    pub(crate) fn augment_demanded(&self) -> bool {
        let augmented = &self.augmented_columns;
        if augmented.is_empty() {
            return true; // un-enumerable augment ("lift all"): always demanded
        }
        match self.demanded_columns() {
            None => true,
            Some(demanded) => augmented.iter().any(|c| demanded.contains(c)),
        }
    }

    /// Spawn the augment-scheduler worker pool if it isn't running yet.
    /// Idempotent and cheap after the first call. Called when an augmented
    /// Dio is built, and again by a two-pass scenery open and a facade read
    /// about to block on hydration, so a Dio without augmentations never runs
    /// a worker task.
    pub(crate) fn ensure_augment_workers(self: &Arc<Self>) {
        let mut guard = self.augment_worker_handles.lock().unwrap();
        if !guard.is_empty() {
            return;
        }
        let workers = self.lens.tuning().augment_workers;
        for _ in 0..workers {
            let weak = Arc::downgrade(self);
            let scheduler = self.augment_scheduler.clone();
            guard.push(
                self.lens
                    .runtime
                    .spawn(augment_scheduler::augment_worker_loop(weak, scheduler)),
            );
        }
    }
}
