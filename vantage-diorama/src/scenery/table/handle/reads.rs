//! Reading the rows a view points at: their load state, one row, their
//! estimated total.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::scenery::enriched_record::EnrichedRecord;
use crate::scenery::table::LoadState;
use crate::scenery::table::cap::Cap;
use crate::scenery::table::plan::Serving;
use crate::scenery::table::state::SharedRows;

/// The [`LoadState`] computation, shared by
/// [`TableScenery::load_state`](crate::TableScenery::load_state) and
/// [`SharedRows::note_state`] so the trait method and the debug stream
/// can never disagree about what state a scenery is in.
pub(crate) fn compute_load_state(state: &SharedRows) -> LoadState {
    if let Serving::Failed(failure) = state.serving() {
        return LoadState::Failed(failure);
    }
    if !state.settled.load(Ordering::SeqCst) {
        return LoadState::Loading;
    }
    if state.omits_failed_rows.load(Ordering::SeqCst) {
        return LoadState::Partial;
    }
    // A known total the visible map has not reached means rows are still
    // coming — the ordinary state of a paged grid before it is scrolled.
    // With no total there is nothing to be short of, so what is loaded is
    // all there is.
    let loaded = state.rows.read().unwrap().len();
    match *state.total.read().unwrap() {
        Some(total) if loaded < total => LoadState::Partial,
        _ => LoadState::Complete,
    }
}

/// The row at `idx` of `shared`, unless a total ends the rows before it.
pub(super) fn shared_row(shared: &SharedRows, idx: usize) -> Option<Arc<EnrichedRecord>> {
    if let Some(t) = *shared.total.read().unwrap()
        && idx >= t
    {
        return None;
    }
    shared.rows.read().unwrap().get(&idx).cloned()
}

/// What the rows themselves estimate their total at.
pub(super) fn shared_estimate(shared: &SharedRows) -> Option<usize> {
    if shared.local_refine() {
        return Some(shared.rows.read().unwrap().len());
    }
    // Two-pass: the running index length is the best estimate; it grows as
    // pages load and freezes once the list pass completes.
    if let Some(index) = shared.index() {
        return Some(index.len());
    }
    let stored = *shared.total.read().unwrap();
    stored.or_else(|| Some(shared.rows.read().unwrap().len()))
}

/// Whether every row the cap shows over `len` shared rows is loaded, so a
/// capped view is complete while its rows hold a larger set only partly.
pub(super) fn cap_loaded(shared: &SharedRows, cap: Cap, len: usize) -> bool {
    !shared.omits_failed_rows.load(Ordering::SeqCst)
        && (0..cap.view_len(len)).all(|i| {
            cap.map(i, len)
                .and_then(|s| shared_row(shared, s))
                .is_some()
        })
}
