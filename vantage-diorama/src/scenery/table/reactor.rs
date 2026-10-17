use std::sync::{Arc, Weak};

use tokio::sync::{Notify, broadcast};

use crate::dio::DioEvent;

use super::state::SharedRows;

/// Background task that reacts to Dio-level events for the rows.
///
/// Single-row events (`RecordChanged`, `RecordInserted`, `RecordRemoved`)
/// splice the one row: into a two-pass index, or into single-pass rows that
/// hold their ordered set whole. Other single-pass rows refresh. Whole-set
/// events (`DatasetChanged`, `Refreshing`) drop the sparse map and re-seed
/// from cache. Our own viewport events
/// (`ViewportChanged`, `RangeLoaded`, `LoadFailed`) are emitted *by* the
/// loader pipeline — the reactor ignores them so it doesn't loop on its own
/// output.
///
/// Holds the rows weakly while it waits, so it never keeps them alive.
pub(crate) async fn reload_loop(
    rows: Weak<SharedRows>,
    reload: Arc<Notify>,
    mut bus: broadcast::Receiver<DioEvent>,
) {
    loop {
        tokio::select! {
            // A re-pointed view pings `reload_notify` on its fresh copy. Two-
            // pass rows must re-point their ordered index and restart
            // hydration (single-pass `reseed_from_cache` re-sorts in memory
            // and is enough on its own).
            _ = reload.notified() => {
                let Some(state) = live(&rows) else {
                    return;
                };
                if state.two_pass {
                    super::two_pass::resort(state.clone()).await;
                } else {
                    reseed(&state).await;
                }
            }
            recv = bus.recv() => {
                let Some(state) = live(&rows) else {
                    return;
                };
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
                    // of re-deriving its whole index. An ordered set held
                    // whole moves the one row to its new place.
                    Ok(DioEvent::RecordChanged { id }) => {
                        if state.two_pass {
                            super::two_pass::update_row_from_cache(&state, &id).await;
                        } else {
                            splice_or_refresh(&state, &id).await;
                        }
                    }
                    // One row joined or left: a two-pass scenery splices its
                    // index in place rather than re-listing it, and an
                    // ordered set held whole splices its rows.
                    Ok(DioEvent::RecordInserted { id }) => {
                        if state.two_pass {
                            super::two_pass::splice_inserted(&state, &id).await;
                        } else {
                            splice_or_refresh(&state, &id).await;
                        }
                    }
                    Ok(DioEvent::RecordRemoved { id }) => {
                        if state.two_pass {
                            super::two_pass::splice_removed(&state, &id).await;
                        } else {
                            splice_or_refresh(&state, &id).await;
                        }
                    }
                    Ok(DioEvent::DatasetChanged) => {
                        refresh(&state).await;
                    }
                    // The eager seed landed in the cache — which matters only
                    // to a scenery whose rows come *from* the cache. A two-pass
                    // scenery's spine is its own ordered index, and a paged
                    // one's is the master's window order, so for them a full
                    // cache is not an answer and must not be treated as one:
                    // reseeding a paged scenery here would paint the cache's
                    // id-keyed order and mark it settled over a set it never
                    // fetched. A shared lens can seed under any of the three
                    // shapes, so the scenery reads its own recorded shape, not
                    // what the lens offers.
                    Ok(DioEvent::Seeded) => {
                        if state.two_pass {
                            super::two_pass::refresh_index(&state).await;
                        } else if !state.is_chunk_loaded() {
                            reseed(&state).await;
                        }
                    }
                    // A scheduled detail fetch for one of our rows failed —
                    // stamp the slot so the grid shows the failure (single-pass
                    // rows load through their own chunk pipeline, not the
                    // scheduler, so only two-pass reacts).
                    // A refined view re-derives its set instead: it no longer
                    // waits for that row before it settles.
                    Ok(DioEvent::RecordLoadFailed { id, error }) => {
                        if state.two_pass && state.local_refine() {
                            super::two_pass::update_row_from_cache(&state, &id).await;
                        } else if state.two_pass {
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
                    // what actually updates the view. A failed follow
                    // re-read keeps the rows on screen, as any failed
                    // refresh does.
                    Ok(DioEvent::WriteFailed { .. })
                    | Ok(DioEvent::ViewportChanged { .. })
                    | Ok(DioEvent::RangeLoaded { .. })
                    | Ok(DioEvent::LoadFailed { .. })
                    | Ok(DioEvent::RefreshFailed { .. })
                    | Ok(DioEvent::Hydrating { .. }) => {}
                    // Dropped events: the reactor fell behind the bus, so it
                    // cannot know what it missed and re-derives the whole set.
                    // Worth a line — an overflow is otherwise indistinguishable
                    // from a legitimate whole-set refresh, and a steady trickle
                    // of them means some producer is outrunning this loop.
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::debug!(
                            target: "vantage_diorama::augment",
                            missed,
                            "event bus lagged — re-deriving the whole set",
                        );
                        refresh(&state).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    }
}

/// The rows, while they and their Dio are alive.
fn live(rows: &Weak<SharedRows>) -> Option<Arc<SharedRows>> {
    let state = rows.upgrade()?;
    state.dio_weak.upgrade()?;
    Some(state)
}

/// Whole-set refresh. A two-pass scenery's row spine is its per-query index,
/// and the cache may have gained or lost rows behind it — re-derive the index
/// from a fresh list pass. A chunk-loaded (paged/lazy) scenery re-fetches its
/// last viewport in place — `force_load` overwrites each slot as fresh rows
/// land and a failed refetch keeps the existing rows, so the grid never
/// blanks, and the refetch restates the total when the loader reports one.
/// Other sceneries reseed the sparse map from the cache (which the refresh has
/// already restaged).
async fn refresh(state: &Arc<SharedRows>) {
    if state.two_pass {
        crate::scenery::table::two_pass::refresh_index(state).await;
    } else if state.is_chunk_loaded() {
        // The poll: nobody typed anything and the rows are already on screen,
        // so this re-pull is background work behind them.
        state.refresh_loaded_viewport(vantage_core::Priority::Background);
    } else {
        reseed(state).await;
    }
}

/// Splice row `id` into single-pass rows that hold their ordered set whole;
/// [`refresh`] the rest, and any rows the splice can't place.
async fn splice_or_refresh(state: &Arc<SharedRows>, id: &str) {
    if state.splices_in_place() {
        match state.splice_one(id).await {
            Ok(true) => return,
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, id, "TableScenery splice failed; reseeding"),
        }
    }
    refresh(state).await;
}

/// Rebuild the sparse map from a fresh cache snapshot, bumping generation on
/// success. Preserves the cache as the source of truth for index assignments.
async fn reseed(state: &Arc<SharedRows>) {
    if let Err(e) = state.reseed_from_cache().await {
        tracing::error!(error = %e, "TableScenery reseed failed");
    } else {
        // A reseed that found rows is an answer. One that found none is only an
        // answer if nothing further is coming:
        //
        // - **serving windows**: the cache is not the row source; the loader
        //   settles the view when the window arrives, which can be after a
        //   reseed triggered at mount.
        // - **eager**: a detached eager start may still be filling the cache,
        //   and an empty cache looks like an empty table until
        //   `seed_complete` says the copy is done.
        //
        // Same rule the open path uses (see `builder`).
        let has_rows = !state.rows.read().unwrap().is_empty();
        let seeded = state
            .dio_weak
            .upgrade()
            .map(|d| d.seed_complete.load(std::sync::atomic::Ordering::SeqCst))
            .unwrap_or(true);
        if has_rows || (!state.is_chunk_loaded() && seeded) {
            state.mark_settled("reactor reseed");
        }
        state.bump_generation();
    }
}
