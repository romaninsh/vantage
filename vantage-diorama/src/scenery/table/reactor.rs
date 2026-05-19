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
            _ = state.reload_notify.notified() => {
                if let Err(e) = state.reseed_from_cache().await {
                    tracing::error!(error = %e, "TableScenery reseed failed");
                } else {
                    state.bump_generation();
                }
            }
            recv = bus.recv() => {
                match recv {
                    Ok(DioEvent::RecordChanged { .. })
                    | Ok(DioEvent::RecordInserted { .. })
                    | Ok(DioEvent::RecordRemoved { .. })
                    | Ok(DioEvent::Invalidated)
                    | Ok(DioEvent::Refreshing) => {
                        // v2 starts with full reseed — preserves the cache as
                        // the source of truth for index assignments. Targeted
                        // single-row updates (preserving chunk-loaded indices)
                        // land in a follow-up iteration.
                        if let Err(e) = state.reseed_from_cache().await {
                            tracing::error!(error = %e, "TableScenery reseed failed");
                        } else {
                            state.bump_generation();
                        }
                    }
                    Ok(DioEvent::WriteFailed { .. })
                    | Ok(DioEvent::ViewportChanged { .. })
                    | Ok(DioEvent::RangeLoaded { .. })
                    | Ok(DioEvent::LoadFailed { .. }) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if let Err(e) = state.reseed_from_cache().await {
                            tracing::error!(error = %e, "TableScenery reseed failed");
                        } else {
                            state.bump_generation();
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    }
}
