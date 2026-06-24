use std::sync::Arc;

use tokio::sync::broadcast;

use crate::dio::DioEvent;

use super::state::TableSceneryState;

/// Background task that reacts to Dio-level events for the scenery.
///
/// Single-row events (`RecordChanged`) update the matching slot in
/// place if known. Whole-set events (`Invalidated`, `Refreshing`)
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
                    // refetch happens on the trailing `Invalidated`. Reseeding
                    // here would only re-show the current cache, so a
                    // chunk-loaded scenery ignores it.
                    Ok(DioEvent::Refreshing) => {
                        if !state.is_chunk_loaded() {
                            reseed(&state).await;
                        }
                    }
                    Ok(DioEvent::RecordChanged { .. })
                    | Ok(DioEvent::RecordInserted { .. })
                    | Ok(DioEvent::RecordRemoved { .. })
                    | Ok(DioEvent::Invalidated) => {
                        refresh(&state).await;
                    }
                    Ok(DioEvent::WriteFailed { .. })
                    | Ok(DioEvent::ViewportChanged { .. })
                    | Ok(DioEvent::RangeLoaded { .. })
                    | Ok(DioEvent::LoadFailed { .. }) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        refresh(&state).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    }
}

/// Whole-set refresh. A chunk-loaded (paged/lazy) scenery re-fetches its
/// last viewport in place — `force_load` overwrites each slot as fresh rows
/// land and a failed refetch keeps the existing rows, so the grid never
/// blanks. Other sceneries reseed the sparse map from the cache (which their
/// `on_refresh` has already restaged).
async fn refresh(state: &Arc<TableSceneryState>) {
    if state.is_chunk_loaded() {
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
