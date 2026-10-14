use std::sync::Arc;

use crate::scenery::enriched_record::EnrichedRecord;

use super::super::super::state::SharedRows;
use super::{run_detail_for_range, sweeps_whole_index};

/// Restart hydration after the rows' index or order changed: re-issue each
/// view's viewport, or, for rows that [sweep](sweeps_whole_index) with no
/// viewport yet, start the sweep at once. Such rows read as loading until
/// their sweep is done, so they can't wait for a scroll; their predicate is
/// the demand.
pub(crate) fn resume_hydration(state: &Arc<SharedRows>) {
    if state.has_viewport() {
        state.refresh_loaded_viewport(vantage_core::Priority::Background);
        return;
    }
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    if !sweeps_whole_index(state, &dio_inner) {
        return;
    }
    let range = 0..state.page_size;
    let sweep = run_detail_for_range(state.clone(), range, state.sweep_ticket.clone());
    state.tasks.hold(dio_inner.lens.runtime.spawn(sweep));
}

/// React to `DioEvent::RecordLoadFailed { id }`: a scheduled detail fetch for
/// a row of ours failed. Stamp the slot so the failure is visible while the
/// partial (list-pass) columns stay on screen. Locally-refined views skip the
/// stamp — their visible set is re-derived wholesale, which the reactor does
/// instead.
pub(crate) fn mark_detail_failed(state: &Arc<SharedRows>, id: &str, error: &str) {
    if state.local_refine() {
        return;
    }
    let Some(i) = state.id_to_idx.read().unwrap().get(id).copied() else {
        return;
    };
    let prev = state.rows.read().unwrap().get(&i).cloned();
    if let Some(prev) = prev {
        let failed =
            EnrichedRecord::detail_failed(prev.record.clone(), error.to_string(), prev.fetched_at);
        state.rows.write().unwrap().insert(i, Arc::new(failed));
        state.bump_generation();
    }
}
