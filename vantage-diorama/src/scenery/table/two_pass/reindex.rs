//! Re-listing a two-pass index after a whole-set change.

use std::sync::Arc;

use crate::dio::DioEvent;
use crate::dio::query_index::QueryIndex;

use super::super::state::SharedRows;
use super::list::{list_page_into, page_limit};
use super::{refine, variant_key};

/// Re-derive the scenery's index from a fresh list pass — the two-pass
/// analogue of a cache reseed. The cache can gain or lose rows behind a
/// built index (a background sync pump appending pages, the refresh pass
/// reconciling against the master), and an index is append-only and possibly
/// already complete — so a refresh REPLACES the shared index for this
/// variant, re-lists, and rebuilds the sparse map in one swap (same shape as
/// [`resort`](super::resort)).
pub(crate) async fn refresh_index(state: &Arc<SharedRows>) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    if !dio_inner.has_dio_augment() {
        return;
    }
    let key = variant_key(state, &dio_inner);

    // Single-flight, shared with the regular list pass. A concurrent page
    // load wins; the next event retries the refresh.
    {
        let mut guard = state.list_in_flight.lock().unwrap();
        if *guard {
            return;
        }
        *guard = true;
    }
    state.publish_view_stats();

    // Build the replacement spine OFF TO THE SIDE: the scenery keeps serving
    // its current index and map while pages land here, so a refresh never
    // passes through an empty row set (which would blank the grid and reset
    // a UI's cursor). Re-list as deep as the user had scrolled — at least as
    // many rows as the old index held — and a refined view to the end, since
    // it evaluates its query over the whole listed set.
    let old = state.index();
    let target = old.as_ref().map(|i| i.len()).unwrap_or(0);
    let fresh = Arc::new(QueryIndex::new());
    // An insert the old index deferred joins the fresh one's end unless the
    // fresh listing brings it; the swap would drop it otherwise.
    if let Some(old) = &old {
        fresh.adopt_deferred(old);
    }
    let limit = page_limit(state, &dio_inner);
    loop {
        let appended = match list_page_into(state, &dio_inner, &fresh, limit).await {
            Ok(listed) => listed.ids,
            Err(e) => {
                state.clear_list_in_flight();
                let _ = dio_inner.event_bus.send(DioEvent::LoadFailed {
                    range: fresh.len()..fresh.len(),
                    error: e.to_string(),
                });
                return;
            }
        };
        let deep_enough = fresh.len() >= target && !state.local_refine();
        if fresh.is_complete() || deep_enough || appended.is_empty() {
            break;
        }
    }
    state.clear_list_in_flight();

    // Register the new spine first: `reseed_filtered` derives the visible set
    // from the scenery's index, so it has to be the fresh one.
    dio_inner
        .query_indexes
        .lock()
        .unwrap()
        .insert(key, fresh.clone());
    state.set_index(Some(fresh.clone()));

    // Publish EXACTLY ONE visible map, built off to the side like the spine
    // above. Writing the raw index-order map first and reseeding a moment
    // later served a refined grid's whole unfiltered candidate set for the
    // length of the reseed's per-id reads, and a repaint landing inside that
    // window showed rows the filter exists to hide.
    refine::publish(state, &dio_inner, &fresh).await;
    state.bump_generation();

    // Resume hydration for what's on screen without waiting for a scroll.
    super::detail::resume_hydration(state);
}
