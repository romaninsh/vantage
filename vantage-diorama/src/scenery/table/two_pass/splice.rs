//! A confirmed insert or delete, spliced into the two-pass index in place of
//! a re-list.
//!
//! Rows of several views may share one index (it is keyed by base
//! conditions and order only), and each keeps its own visible map. The first
//! reactor to see the event changes the index; every one republishes its map.

use std::sync::Arc;

use super::super::state::SharedRows;
use super::refine;

/// The two-pass answer to `RecordInserted`: the id joins the end of the
/// index, where the master puts a new row, and the visible map is
/// republished. An index still being listed holds the id until its listing
/// ends instead, since a pushed id would move the next page's offset past a
/// row not yet served; the listing brings it, or puts it at the end.
pub(crate) async fn splice_inserted(state: &Arc<SharedRows>, id: &str) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    let Some(index) = state.index() else {
        return;
    };
    if !index.is_complete() {
        index.defer_insert(id.to_string());
        return;
    }
    index.push(id.to_string());
    refine::publish(state, &dio_inner, &index).await;
    state.bump_generation();
}

/// The two-pass answer to `RecordRemoved`: the id leaves the index, and the
/// visible map is republished when the index held the id or the map showed
/// it. The master loses the row too, so a partial index's next page offset
/// still lines up.
pub(crate) async fn splice_removed(state: &Arc<SharedRows>, id: &str) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    let Some(index) = state.index() else {
        return;
    };
    let removed = index.remove(id).is_some();
    let shown = state.id_to_idx.read().unwrap().contains_key(id);
    if !removed && !shown {
        return;
    }
    refine::publish(state, &dio_inner, &index).await;
    state.bump_generation();
}
