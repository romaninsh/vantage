//! The live-scenery registry on [`DioInner`]: dedup, census, demand, and the
//! augment worker pool the open sceneries drive.

use std::collections::HashSet;
use std::sync::Arc;

use super::DioInner;
use crate::dio::augment_scheduler;
use crate::scenery::TableScenery;

impl DioInner {
    /// How many table sceneries have at least one live handle. Reading never
    /// changes the registry: dead entries are pruned only when a scenery is
    /// registered.
    pub(crate) fn live_table_scenery_count(&self) -> usize {
        let guard = self.table_sceneries.lock().unwrap();
        guard
            .values()
            .filter(|weak| weak.strong_count() > 0)
            .count()
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
        let record_sceneries = self
            .record_census
            .load(std::sync::atomic::Ordering::Relaxed);
        let servos = self.servo_census.load(std::sync::atomic::Ordering::Relaxed);
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

    /// The union of every LIVE open scenery's demanded columns — the Dio's
    /// active demand. `None` = at least one open view demands everything (or
    /// no view declared a demand): the pre-demand behavior. `Some(union)` =
    /// only these columns are looked at; the augment detail pass runs only
    /// while the union intersects the augment columns. Recomputed on the fly
    /// from the dedup registry's live entries, so demand follows scenery
    /// open/close with no extra lifecycle machinery.
    pub(crate) fn demanded_columns(&self) -> Option<HashSet<String>> {
        let mut union = HashSet::new();
        let mut any_live = false;
        let guard = self.table_sceneries.lock().unwrap();
        for weak in guard.values() {
            let Some(scenery) = weak.upgrade() else {
                continue;
            };
            any_live = true;
            union.extend(scenery.demanded_columns()?);
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

    /// Return the live shared table scenery for `key`, or `None` if none is
    /// open (or the last handle was just released — a dead `Weak`).
    pub(crate) fn lookup_table_scenery(&self, key: &str) -> Option<Arc<dyn TableScenery>> {
        self.table_sceneries
            .lock()
            .unwrap()
            .get(key)
            .and_then(std::sync::Weak::upgrade)
    }

    /// Publish a freshly-built scenery under `key`. If a concurrent open won
    /// the race for the same key, returns that shared scenery instead and lets
    /// `built` drop — its guard aborts the now-redundant tasks. Otherwise
    /// inserts a `Weak` to `built` and hands it back, dropping the entries of
    /// sceneries every holder has released.
    pub(crate) fn register_table_scenery(
        &self,
        key: String,
        built: Arc<dyn TableScenery>,
    ) -> Arc<dyn TableScenery> {
        let mut guard = self.table_sceneries.lock().unwrap();
        if let Some(existing) = guard.get(&key).and_then(std::sync::Weak::upgrade) {
            return existing;
        }
        guard.retain(|_, weak| weak.strong_count() > 0);
        guard.insert(key, Arc::downgrade(&built));
        built
    }
}
