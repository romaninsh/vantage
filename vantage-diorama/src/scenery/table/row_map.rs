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
#[derive(Default)]
pub(crate) struct RowMap {
    rows: BTreeMap<usize, Row>,
    summary: RowStatusSummary,
}

impl RowMap {
    /// Take over a freshly built map, counting it once.
    pub(crate) fn from_map(rows: BTreeMap<usize, Row>) -> Self {
        #[cfg(test)]
        tests::note_rescan();
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
    let bucket = match status {
        RowStatus::Fresh => Some(&mut summary.fresh),
        RowStatus::Incomplete => Some(&mut summary.incomplete),
        RowStatus::PendingWrite => Some(&mut summary.pending_write),
        RowStatus::LoadFailed { .. } | RowStatus::WriteFailed { .. } => Some(&mut summary.failed),
        _ => None,
    };
    for count in std::iter::once(&mut summary.loaded).chain(bucket) {
        *count = count.wrapping_add_signed(delta);
    }
}

#[cfg(test)]
mod tests;
