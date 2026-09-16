//! The viewport → chunk-load pipeline.
//!
//! - [`viewport_loop`] debounces viewport requests, races the load against
//!   the channel, and decides what is cancelled, absorbed or parked.
//! - [`dispatch::run_chunk_callback`] is the cancellable half: it picks the
//!   effective range, dispatches `on_load_chunk` and returns a
//!   [`guards::PendingChunk`].
//! - [`commit::finish_chunk_load`] is the uncancellable half: it flushes the
//!   page, infers the total, re-sorts and bumps the generation.
//! - [`range`], [`guards`] and [`telemetry`] hold the range arithmetic, the
//!   two RAII guards and the tap/tracing formatting the commit path emits.

mod commit;
mod dispatch;
mod guards;
mod range;
mod telemetry;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::mpsc;

use super::ViewportRequest;
use super::state::TableSceneryState;

use commit::finish_chunk_load;
use dispatch::run_chunk_callback;

/// Debounces viewport requests and fires chunk loads. Restarts the
/// debounce timer on every new request — rapid scroll bursts coalesce
/// into a single fetch for the most recent range.
pub(crate) async fn viewport_loop(
    state: Arc<TableSceneryState>,
    mut rx: mpsc::UnboundedReceiver<ViewportRequest>,
    debounce: Duration,
) {
    // A request that superseded a running load is served next without
    // waiting on the channel again. It re-enters the debounce below rather
    // than firing straight away, on purpose: the scroll burst that
    // superseded the previous load is usually still arriving, and the
    // carried request is only its leading edge.
    let mut carried: Option<ViewportRequest> = None;
    loop {
        if state.dio_weak.upgrade().is_none() {
            tracing::warn!(target: "vantage_diorama::viewport", "viewport_loop: dio dropped, exiting");
            return;
        }
        let initial = match carried.take() {
            Some(req) => req,
            None => {
                let Some(req) = rx.recv().await else {
                    tracing::warn!(target: "vantage_diorama::viewport", "viewport_loop: channel closed, exiting");
                    return;
                };
                state.viewport_queue_depth.fetch_sub(1, Ordering::SeqCst);
                req
            }
        };
        // The pop above already decremented the producer-side counter (a
        // carried request decremented it back when it first arrived); read
        // its current value for the tracing line below.
        let depth_on_pop = state.viewport_queue_depth.load(Ordering::SeqCst);
        let mut latest = initial;
        let mut absorbed = 0usize;

        // Keep absorbing requests until the channel is quiet for `debounce`.
        //
        // Absorbing takes the newest RANGE — an older viewport is not worth
        // fetching — but `force_load` is not a property of the range, it is an
        // instruction to consult the master rather than the cache. Dropping it
        // with the request that carried it lost the on-open refresh on every
        // mount, because the grid always pushes its first real viewport inside
        // the debounce window: the table then sat on cached rows and never
        // contacted the source at all.
        //
        // `priority` merges the same way and for the same reason: it describes
        // the *wait*, not the range. An `Essential` scroll absorbed together
        // with a `Background` refresh still has a user waiting on the rows, and
        // running the merged fetch as `Background` would leave that user
        // without a retry under a transport that only retries what is awaited.
        loop {
            match tokio::time::timeout(debounce, rx.recv()).await {
                Ok(Some(next)) => {
                    state.viewport_queue_depth.fetch_sub(1, Ordering::SeqCst);
                    absorbed += 1;
                    let force = latest.force_load || next.force_load;
                    let essential = latest.priority.is_essential() || next.priority.is_essential();
                    latest = next;
                    latest.force_load = force;
                    if essential {
                        latest.priority = vantage_core::Priority::Essential;
                    }
                }
                Ok(None) => {
                    tracing::warn!(target: "vantage_diorama::viewport", "viewport_loop: channel closed mid-debounce, exiting");
                    return;
                }
                Err(_) => break,
            }
        }
        let depth_after = state.viewport_queue_depth.load(Ordering::SeqCst);
        if absorbed > 0 {
            crate::debug::tapline!(
                state.debug_tap,
                "viewport",
                "rows {}..{} — {} scroll events coalesced into one",
                latest.range.start,
                latest.range.end,
                absorbed + 1,
            );
        }
        tracing::debug!(
            target: "vantage_diorama::viewport",
            range = ?latest.range,
            force_load = latest.force_load,
            priority = ?latest.priority,
            absorbed,
            depth_on_pop,
            depth_after,
            "viewport_loop: firing",
        );
        // Race the callback against the channel, but only up to the point it
        // resolves: a newer viewport for a different range drops it (via
        // `CancelOnDrop`, restoring whatever rows it had bound — nothing is
        // committed) and is served next; an identical range is absorbed and
        // the callback continues; an identical range asking for a re-pull is
        // parked in `after` and runs once this one finishes. Once the
        // callback resolves, its commit and bookkeeping (`finish_chunk_load`)
        // run to completion outside the select — a supersede can no longer
        // reach it once the master has answered.
        let load = run_chunk_callback(state.clone(), latest.clone());
        tokio::pin!(load);
        let mut after: Option<ViewportRequest> = None;
        let pending = loop {
            tokio::select! {
                pending = &mut load => break pending,
                next = rx.recv() => match next {
                    None => {
                        tracing::warn!(target: "vantage_diorama::viewport", "viewport_loop: channel closed mid-load, exiting");
                        return;
                    }
                    Some(mut req) => {
                        state.viewport_queue_depth.fetch_sub(1, Ordering::SeqCst);
                        if req.range != latest.range {
                            tracing::debug!(
                                target: "vantage_diorama::viewport",
                                superseded = ?latest.range,
                                by = ?req.range,
                                "viewport_loop: cancelling the in-flight load",
                            );
                            // A parked same-range refresh is moot against the
                            // range it was for, but the *intent* — force a
                            // re-pull rather than trust the cache, and the
                            // wait behind it — still applies to whatever range
                            // wins next. Same OR as the debounce coalescing,
                            // for the same reason: dropping an `Essential`
                            // here would leave the request someone is waiting
                            // on running as a poll.
                            if let Some(parked) = after.as_ref() {
                                req.force_load |= parked.force_load;
                                if parked.priority.is_essential() {
                                    req.priority = vantage_core::Priority::Essential;
                                }
                            }
                            carried = Some(req);
                            break None; // dropping `load` cancels the callback
                        }
                        if req.force_load {
                            after = Some(req);
                        }
                        // else: same range, already loading — absorbed.
                    }
                },
            }
        };
        carried = carried.or(after);
        if let Some(pending) = pending {
            finish_chunk_load(&state, pending).await;
        }
    }
}

/// Put a viewport request on the debounce channel, keeping the queue-depth
/// counter honest.
pub(crate) fn enqueue_viewport(state: &TableSceneryState, request: ViewportRequest) {
    // Bump the depth counter *before* sending so a racing consumer
    // can't observe a depth lower than what's actually in the channel.
    state.viewport_queue_depth.fetch_add(1, Ordering::SeqCst);
    if state.viewport_tx.send(request).is_err() {
        // Channel closed (Dio dropped). Reverse the bump so the
        // counter doesn't drift upward forever.
        state.viewport_queue_depth.fetch_sub(1, Ordering::SeqCst);
    }
}
