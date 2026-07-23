use std::sync::Arc;

use tokio::sync::broadcast;

use crate::dio::DioEvent;

use super::state::TableSceneryState;

/// Background task that reacts to Dio-level events for the scenery.
///
/// Single-row events (`RecordChanged`) update the matching slot in
/// place if known. Whole-set events (`DatasetChanged`, `Refreshing`)
/// drop the sparse map and re-seed from cache. Our own viewport
/// events (`ViewportChanged`, `RangeLoaded`, `LoadFailed`) are
/// emitted *by* the loader pipeline — the reactor ignores them so
/// it doesn't loop on its own output.
pub(crate) async fn reload_loop(
    state: Arc<TableSceneryState>,
    mut bus: broadcast::Receiver<DioEvent>,
) {
    loop {
        if state.dio_weak.upgrade().is_none() {
            return;
        }

        tokio::select! {
            // `set_sort` / `set_search` ping `reload_notify`. A two-pass
            // scenery must rebuild its ordered index and restart hydration
            // (single-pass `reseed_from_cache` re-sorts in memory and is
            // enough on its own).
            _ = state.reload_notify.notified() => {
                if state.two_pass {
                    super::two_pass::resort(state.clone()).await;
                } else {
                    reseed(&state).await;
                }
            }
            recv = bus.recv() => {
                match recv {
                    // `Refreshing` is the leading edge of `dio.refresh()`; the
                    // refetch happens on the trailing `DatasetChanged`. Reseeding
                    // here would only re-show the current cache, so chunk-loaded
                    // and two-pass sceneries (whose spine is the index, not the
                    // cache's iteration order) ignore it.
                    Ok(DioEvent::Refreshing) => {
                        if !state.is_chunk_loaded() && !state.two_pass {
                            reseed(&state).await;
                        }
                    }
                    // A changed record moves values, not membership — a
                    // two-pass scenery updates that one slot in place instead
                    // of re-deriving its whole index.
                    Ok(DioEvent::RecordChanged { id }) => {
                        if state.two_pass {
                            super::two_pass::update_row_from_cache(&state, &id).await;
                        } else {
                            refresh(&state).await;
                        }
                    }
                    Ok(DioEvent::RecordInserted { .. }
                    | DioEvent::RecordRemoved { .. }
                    | DioEvent::DatasetChanged) => {
                        refresh(&state).await;
                    }
                    // A scheduled detail fetch for one of our rows failed —
                    // stamp the slot so the grid shows the failure (single-pass
                    // rows load through their own chunk pipeline, not the
                    // scheduler, so only two-pass reacts).
                    Ok(DioEvent::RecordLoadFailed { id, error }) => {
                        if state.two_pass {
                            super::two_pass::mark_detail_failed(&state, &id, &error);
                        }
                    }
                    // Optimistic-write affordance: stamp just the affected row
                    // rather than reseeding the whole map. On success the
                    // trailing `RecordChanged` settles it back to `Fresh`.
                    Ok(DioEvent::WritePending { id, .. }) => {
                        state.mark_row(&id, crate::scenery::RowStatus::PendingWrite).await;
                    }
                    Ok(DioEvent::WriteReverted { id, error, .. }) => {
                        state
                            .mark_row(&id, crate::scenery::RowStatus::WriteFailed { error })
                            .await;
                    }
                    // A facade read announcing its hydration sweep; each
                    // hydrated row follows as `RecordChanged`, which is
                    // what actually updates the view.
                    Ok(DioEvent::WriteFailed { .. })
                    | Ok(DioEvent::ViewportChanged { .. })
                    | Ok(DioEvent::RangeLoaded { .. })
                    | Ok(DioEvent::LoadFailed { .. })
                    | Ok(DioEvent::Hydrating { .. }) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        refresh(&state).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    }
}

/// Whole-set refresh. A two-pass scenery's row spine is its per-query index,
/// and the cache may have gained or lost rows behind it — re-derive the index
/// from a fresh list pass. A chunk-loaded (paged/lazy) scenery re-fetches its
/// last viewport in place — `force_load` overwrites each slot as fresh rows
/// land and a failed refetch keeps the existing rows, so the grid never
/// blanks. Other sceneries reseed the sparse map from the cache (which their
/// `on_refresh` has already restaged).
async fn refresh(state: &Arc<TableSceneryState>) {
    if state.two_pass {
        crate::scenery::table::two_pass::refresh_index(state).await;
    } else if state.is_chunk_loaded() {
        // Re-count first: a chunk-loaded scenery caches its total at open and
        // would otherwise never notice a row that appeared (or vanished)
        // server-side. Then re-fetch the current viewport in place.
        state.refresh_total().await;
        state.refresh_loaded_viewport();
    } else {
        reseed(state).await;
    }
}

/// Rebuild the sparse map from a fresh cache snapshot, bumping generation on
/// success. Preserves the cache as the source of truth for index assignments.
async fn reseed(state: &Arc<TableSceneryState>) {
    if let Err(e) = state.reseed_from_cache().await {
        tracing::error!(error = %e, "TableScenery reseed failed");
    } else {
        state.bump_generation();
    }
}
