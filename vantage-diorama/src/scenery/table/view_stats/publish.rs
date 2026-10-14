//! Computing and publishing a [`SharedRows`]' [`ViewStats`].
//!
//! Every path that can move a number in the snapshot calls
//! [`publish_view_stats`](SharedRows::publish_view_stats): the generation
//! bump (rows landed, reseeded, re-ordered), a total change, and a fetch
//! starting or ending. Publishing recomputes the snapshot and signals the
//! attached views only when it differs, so a refresh that changes nothing
//! wakes no one. The row counts come from the row map's running tally, so a
//! publish costs the same however many rows are loaded. Each view lays its
//! own shown range and filters over the snapshot as it reads it.

use std::sync::Mutex;
use std::sync::RwLock;

use crate::scenery::table::state::SharedRows;
use crate::scenery::table::{LoadState, compute_load_state};

use super::{ViewState, ViewStats};

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

/// The rows' published snapshot.
pub(crate) struct StatsCell {
    snapshot: RwLock<ViewStats>,
    /// Held while computing and storing, so two publishers can't store their
    /// snapshots in the opposite order to the one they computed them in.
    publishing: Mutex<()>,
}

impl StatsCell {
    pub(crate) fn new() -> Self {
        Self {
            snapshot: RwLock::new(ViewStats::default()),
            publishing: Mutex::new(()),
        }
    }

    pub(crate) fn snapshot(&self) -> ViewStats {
        self.snapshot.read().unwrap().clone()
    }
}

impl SharedRows {
    /// Recompute the snapshot and signal the attached views if it changed.
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
        for signal in self.signals() {
            signal.bump_stats();
        }
    }

    /// The snapshot without the shown range and filters, which are each
    /// view's own.
    fn compute_view_stats(&self) -> ViewStats {
        let load = compute_load_state(self);
        let rows = self.status_summary();
        let (total, total_exact) = self.stats_total(rows.loaded, &load);
        let fetching =
            self.load_in_flight.lock().unwrap().is_some() || *self.list_in_flight.lock().unwrap();
        ViewStats {
            total,
            total_exact,
            loaded: rows.loaded,
            showing: None,
            has_more: self.has_more(),
            order: self.query.order.clone(),
            search: self.query.search.clone(),
            filters: Vec::new(),
            state: ViewState::derive(&load, &rows, fetching),
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
    fn stats_total(&self, loaded: usize, load: &LoadState) -> (Option<usize>, bool) {
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
            (None, _)
                if !self.is_chunk_loaded()
                    && matches!(load, LoadState::Partial | LoadState::Complete) =>
            {
                (Some(loaded), true)
            }
            (None, _) => (None, false),
        }
    }
}
