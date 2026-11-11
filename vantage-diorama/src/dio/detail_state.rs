//! What the detail pass last concluded about each row.
//!
//! The sweep can only tell a settled row from the record, at a cache read per
//! row. On a view whose query forces whole-index hydration that is a read of
//! every listed row on every pass. The state records the answer instead, so
//! a settled row costs one lookup. It memoizes the sweep's whole skip rule
//! (`Complete` and no augment gap), not the status alone: a cache warmed by
//! plain inserts stores rows `Complete` without a detail pass.
//!
//! A refresh demotes changed rows to `Incomplete` to make the pass fetch them
//! again, so every refresh forgets `Filled` and `Failed`. `Empty` survives a
//! scheduled one: a timed re-pull of the master is no evidence the detail
//! source gained a record, and re-probing every empty row on each tick keeps
//! the sweep from ever finishing. A requested refresh forgets it too, and an
//! answer from a fetch that began before the request doesn't count.

use std::collections::HashMap;

use super::DioInner;

/// The detail pass's last answer for one row. A row holds at most one; a row
/// with none hasn't settled since the last refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailState {
    /// Hydrated, every augmented column filled.
    Filled,
    /// Hydrated, and the detail source has no record to fill it: the gap is
    /// the row's real value.
    Empty,
    /// The last detail fetch failed. The sweep retries it; a clause on an
    /// augmented value doesn't judge it.
    Failed,
}

/// Every row's [`DetailState`], and how many requested refreshes have
/// forgotten them.
#[derive(Debug, Default)]
pub(crate) struct DetailStates {
    rows: HashMap<String, DetailState>,
    requested: u64,
}

impl DioInner {
    pub(crate) fn detail_state(&self, id: &str) -> Option<DetailState> {
        self.detail_state.lock().unwrap().rows.get(id).copied()
    }

    pub(crate) fn set_detail_state(&self, id: &str, state: DetailState) {
        self.detail_state
            .lock()
            .unwrap()
            .rows
            .insert(id.to_string(), state);
    }

    /// The count of requested refreshes so far. A detail fetch reads it
    /// before it starts and hands it to [`settle_detail`](Self::settle_detail).
    pub(crate) fn detail_epoch(&self) -> u64 {
        self.detail_state.lock().unwrap().requested
    }

    /// Record `id`'s answer from a fetch that began at `epoch`, unless a
    /// requested refresh came since: the fetch may predate a record the
    /// refresh was asked to find, so the row stays unsettled. `false` when
    /// the answer wasn't recorded.
    pub(crate) fn settle_detail(&self, id: &str, state: DetailState, epoch: u64) -> bool {
        let mut states = self.detail_state.lock().unwrap();
        let current = states.requested == epoch;
        if current {
            states.rows.insert(id.to_string(), state);
        }
        current
    }

    /// Forget `id`'s `Filled`: the list pass wrote its list row back over the
    /// detail, so the row has to be read again.
    pub(crate) fn forget_filled(&self, id: &str) {
        let mut states = self.detail_state.lock().unwrap();
        if states.rows.get(id) == Some(&DetailState::Filled) {
            states.rows.remove(id);
        }
    }

    /// Forget every answer, or every one but `Empty` when `keep_empty` (a
    /// scheduled refresh).
    pub(crate) fn clear_detail_states(&self, keep_empty: bool) {
        let mut states = self.detail_state.lock().unwrap();
        if keep_empty {
            states.rows.retain(|_, state| *state == DetailState::Empty);
        } else {
            states.rows.clear();
            states.requested += 1;
        }
    }
}
