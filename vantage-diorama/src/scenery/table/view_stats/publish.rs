//! Computing and publishing a [`TableSceneryState`]'s [`ViewStats`].
//!
//! Every path that can move a number in the snapshot calls
//! [`publish_view_stats`](TableSceneryState::publish_view_stats): the
//! generation bump (rows landed, reseeded, re-ordered), a total change, the
//! shown range, a sort / search / filter change, and a fetch starting or
//! ending. Publishing recomputes the snapshot and signals subscribers only
//! when it differs, so a refresh that changes nothing wakes no one. The row
//! counts come from the row map's running tally, so a publish costs the same however many rows are loaded.

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

use tokio::sync::watch;

use crate::dio::Generation;
use crate::scenery::table::state::TableSceneryState;
use crate::scenery::table::{LoadState, compute_load_state};

use super::{ViewState, ViewStats, clamp_range, view_filters};

/// How much the stored grand total can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TotalKind {
    /// Stated by the source, or the end of the set was reached.
    Exact,
    /// Stated in an earlier session and restored from the cache; the next
    /// stated total replaces it.
    Remembered,
    /// The unknown-total horizon: loaded rows plus one page, so scrolling can
    /// keep asking. A scroll affordance, not a total.
    Horizon,
}

/// A scenery's published snapshot, the consumer's viewport, and the
/// signal that tells subscribers the snapshot moved.
pub(crate) struct StatsCell {
    snapshot: RwLock<ViewStats>,
    /// Held while computing and storing, so two publishers can't store their
    /// snapshots in the opposite order to the one they computed them in.
    publishing: Mutex<()>,
    /// The rows a displaying consumer last declared on screen with
    /// `set_shown_range`. Independent of the hydration viewport: a dashboard
    /// counter or an observation probe sets a viewport to load rows without
    /// showing any of them.
    shown: RwLock<Option<Range<usize>>>,
    generation: AtomicU64,
    tx: watch::Sender<Generation>,
}

impl StatsCell {
    pub(crate) fn new() -> Self {
        Self {
            snapshot: RwLock::new(ViewStats::default()),
            publishing: Mutex::new(()),
            shown: RwLock::new(None),
            generation: AtomicU64::new(0),
            tx: watch::channel(Generation::default()).0,
        }
    }

    pub(crate) fn snapshot(&self) -> ViewStats {
        self.snapshot.read().unwrap().clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Generation> {
        self.tx.subscribe()
    }
}

impl TableSceneryState {
    /// Recompute the snapshot and signal subscribers if it changed.
    ///
    /// Takes the row, total and query locks for reading; never call it while
    /// holding any of them for writing.
    pub(crate) fn publish_view_stats(&self) {
        let _publishing = self.stats.publishing.lock().unwrap();
        let next = self.compute_view_stats();
        {
            let mut current = self.stats.snapshot.write().unwrap();
            if *current == next {
                return;
            }
            *current = next;
        }
        let generation = self.stats.generation.fetch_add(1, Ordering::SeqCst) + 1;
        // `send_replace` so a consumer subscribing later still reads the
        // latest generation, even with no receiver alive right now.
        self.stats.tx.send_replace(Generation(generation));
    }

    /// Record the rows a displaying consumer has on screen and republish.
    pub(crate) fn set_shown_range(&self, range: Option<Range<usize>>) {
        *self.stats.shown.write().unwrap() = range;
        self.publish_view_stats();
    }

    fn compute_view_stats(&self) -> ViewStats {
        let load = compute_load_state(self);
        let rows = self.status_summary();
        let (total, total_exact) = self.stats_total(rows.loaded, load);
        let fetching =
            self.load_in_flight.lock().unwrap().is_some() || *self.list_in_flight.lock().unwrap();
        let shown = self.stats.shown.read().unwrap().clone();
        ViewStats {
            total,
            total_exact,
            loaded: rows.loaded,
            showing: clamp_range(shown, self.row_count()),
            has_more: self.has_more(),
            order: self.sort.read().unwrap().clone(),
            search: self.search.read().unwrap().clone(),
            filters: view_filters(
                &self.conditions.read().unwrap(),
                &self.op_conditions.read().unwrap(),
                &self.ui_filters.read().unwrap(),
                &self.ui_terms.read().unwrap(),
            ),
            state: ViewState::derive(load, &rows, fetching),
            pending_writes: rows.pending_write,
            failed: rows.failed,
        }
    }

    /// The grand total and whether it is exact, or `None` where only a lower
    /// bound is known.
    ///
    /// A two-pass index still listing is a lower bound, not an estimate: its
    /// length is what has been listed so far. A view the cache holds whole
    /// counts its own rows once it has an answer.
    fn stats_total(&self, loaded: usize, load: LoadState) -> (Option<usize>, bool) {
        if let Some(index) = self.index() {
            let len = if self.local_refine() {
                loaded
            } else {
                index.len()
            };
            return if index.is_complete() {
                (Some(len), true)
            } else {
                (None, false)
            };
        }
        match (
            *self.total.read().unwrap(),
            *self.total_kind.read().unwrap(),
        ) {
            (Some(total), TotalKind::Exact) => (Some(total), true),
            (Some(total), TotalKind::Remembered) => (Some(total), false),
            (Some(_), TotalKind::Horizon) => (None, false),
            (None, _) if !self.paged && load != LoadState::Loading => (Some(loaded), true),
            (None, _) => (None, false),
        }
    }
}
