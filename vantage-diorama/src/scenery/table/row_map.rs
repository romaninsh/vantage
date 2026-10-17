//! The sparse row map a table view shows, with its status breakdown kept
//! current on every write.
//!
//! [`ViewStats`](super::ViewStats) is republished on every generation bump,
//! and a two-pass hydration bumps once per row. Counting statuses by walking
//! the map there made hydrating N rows cost O(N²); keeping the counts beside
//! the map makes reading them O(1).

use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::Arc;

use super::RowStatusSummary;
use crate::scenery::enriched_record::{EnrichedRecord, RowStatus};

type Row = Arc<EnrichedRecord>;

/// A `BTreeMap<usize, Arc<EnrichedRecord>>` that counts its rows by status.
///
/// Reads go through `Deref`; every write goes through a method here so the
/// counts can't drift from the map.
#[derive(Default, Clone)]
pub(crate) struct RowMap {
    rows: BTreeMap<usize, Row>,
    summary: RowStatusSummary,
}

impl RowMap {
    /// Take over a freshly built map, counting it once.
    pub(crate) fn from_map(rows: BTreeMap<usize, Row>) -> Self {
        let mut summary = RowStatusSummary::default();
        for row in rows.values() {
            tally(&mut summary, &row.status, 1);
        }
        Self { rows, summary }
    }

    /// The status breakdown of the rows held now.
    pub(crate) fn summary(&self) -> RowStatusSummary {
        self.summary.clone()
    }

    pub(crate) fn insert(&mut self, idx: usize, row: Row) -> Option<Row> {
        tally(&mut self.summary, &row.status, 1);
        let previous = self.rows.insert(idx, row);
        if let Some(old) = &previous {
            tally(&mut self.summary, &old.status, -1);
        }
        previous
    }

    pub(crate) fn remove(&mut self, idx: &usize) -> Option<Row> {
        let previous = self.rows.remove(idx);
        if let Some(old) = &previous {
            tally(&mut self.summary, &old.status, -1);
        }
        previous
    }

    /// Put `row` at `idx`, moving every row at `idx` or past it up by one.
    pub(crate) fn insert_shift(&mut self, idx: usize, row: Row) {
        let moved = self.rows.split_off(&idx);
        tally(&mut self.summary, &row.status, 1);
        self.rows.insert(idx, row);
        self.rows
            .extend(moved.into_iter().map(|(at, row)| (at + 1, row)));
    }

    /// Take the row at `idx` out, moving every row past it down by one.
    pub(crate) fn remove_shift(&mut self, idx: usize) -> Option<Row> {
        let moved = self.rows.split_off(&(idx + 1));
        let removed = self.remove(&idx);
        self.rows
            .extend(moved.into_iter().map(|(at, row)| (at - 1, row)));
        removed
    }

    /// Drop every row at `len` or past it, returning them.
    pub(crate) fn truncate(&mut self, len: usize) -> BTreeMap<usize, Row> {
        let dropped = self.rows.split_off(&len);
        for row in dropped.values() {
            tally(&mut self.summary, &row.status, -1);
        }
        dropped
    }

    pub(crate) fn clear(&mut self) {
        self.rows.clear();
        self.summary = RowStatusSummary::default();
    }
}

impl Deref for RowMap {
    type Target = BTreeMap<usize, Row>;

    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}

/// Add (`delta = 1`) or remove (`delta = -1`) one row of `status`.
fn tally(summary: &mut RowStatusSummary, status: &RowStatus, delta: isize) {
    summary.loaded = summary.loaded.wrapping_add_signed(delta);
    let bucket = match status {
        RowStatus::Fresh => &mut summary.fresh,
        RowStatus::Incomplete => &mut summary.incomplete,
        RowStatus::PendingWrite => &mut summary.pending_write,
        RowStatus::LoadFailed { .. } | RowStatus::WriteFailed { .. } => &mut summary.failed,
    };
    *bucket = bucket.wrapping_add_signed(delta);
}
