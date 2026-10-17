//! The viewport → chunk-load pipeline.
//!
//! - [`viewport_loop`] debounces viewport requests, races the load against
//!   the channel, and decides what is cancelled, absorbed or parked.
//! - [`dispatch::run_chunk_callback`] is the cancellable half: it picks the
//!   effective range, dispatches the paged loader and returns a
//!   [`guards::PendingChunk`].
//! - [`commit::finish_chunk_load`] is the uncancellable half: it flushes the
//!   page, infers the total, re-sorts and bumps the generation.
//! - [`range`], [`guards`] and [`telemetry`] hold the choice of what to
//!   fetch, the in-flight claim with its two RAII guards, and the tap/tracing
//!   formatting dispatch and commit emit.

mod commit;
mod dispatch;
mod guards;
mod range;
mod telemetry;

use std::sync::Weak;
use std::sync::atomic::Ordering;
use std::time::Duration;

use indexmap::IndexMap;
use tokio::sync::mpsc;

use super::ViewportRequest;
use super::state::SharedRows;
use super::view::ViewId;

use commit::finish_chunk_load;
use dispatch::run_chunk_callback;

/// The requests waiting to be served: one per view (and one for the requests
/// no view made), in the order each first asked.
type Pending = IndexMap<Option<ViewId>, ViewportRequest>;

/// Debounces viewport requests and fires chunk loads. Restarts the
/// debounce timer on every new request — rapid scroll bursts coalesce
/// into a single fetch for each view's most recent range.
///
/// Holds the rows weakly while it waits, so it never keeps them alive; the
/// rows' last view detaching aborts it, mid-debounce or mid-load, and it
/// exits when they drop (their sender closes the channel).
///
/// Once the channel has been quiet for one debounce, every request already
/// pending is served in turn without a fresh one.
pub(crate) async fn viewport_loop(
    rows: Weak<SharedRows>,
    mut rx: mpsc::UnboundedReceiver<ViewportRequest>,
    debounce: Duration,
) {
    // A request that superseded a running load stays at the front of
    // `pending` and is served next without waiting on the channel again. It
    // re-enters the debounce below rather than firing straight away, on
    // purpose: the scroll burst that superseded the previous load is usually
    // still arriving, and the carried request is only its leading edge.
    let mut pending = Pending::new();
    let mut debounce_next = true;
    loop {
        if pending.is_empty() {
            let Some(req) = rx.recv().await else {
                tracing::debug!(target: "vantage_diorama::viewport", "viewport_loop: channel closed, exiting");
                return;
            };
            let Some(state) = rows.upgrade() else {
                return;
            };
            state.viewport_queue_depth.fetch_sub(1, Ordering::SeqCst);
            merge(&mut pending, req);
            debounce_next = true;
        }
        let Some(state) = rows.upgrade() else {
            return;
        };
        if state.dio_weak.upgrade().is_none() {
            tracing::warn!(target: "vantage_diorama::viewport", "viewport_loop: dio dropped, exiting");
            return;
        }
        // The pop above already decremented the producer-side counter (a
        // carried request decremented it back when it first arrived); read
        // its current value for the tracing line below.
        let depth_on_pop = state.viewport_queue_depth.load(Ordering::SeqCst);
        let mut absorbed = 0usize;

        // Keep absorbing requests until the channel is quiet for `debounce`.
        //
        // Absorbing takes each view's newest RANGE — an older viewport is not
        // worth fetching — but `force_load` is not a property of the range, it
        // is an instruction to consult the master rather than the cache, so
        // it is ORed in: a grid pushes its first real viewport inside the
        // debounce window, and the on-open refresh must survive that.
        //
        // `priority` merges the same way and for the same reason: it describes
        // the *wait*, not the range. An `Essential` scroll absorbed together
        // with a `Background` refresh still has a user waiting on the rows, and
        // running the merged fetch as `Background` would leave that user
        // without a retry under a transport that only retries what is awaited.
        //
        // Requests from different views never absorb each other: each view
        // keeps its own viewport, and both are served.
        if debounce_next {
            loop {
                match tokio::time::timeout(debounce, rx.recv()).await {
                    Ok(Some(next)) => {
                        state.viewport_queue_depth.fetch_sub(1, Ordering::SeqCst);
                        absorbed += 1;
                        merge(&mut pending, next);
                    }
                    Ok(None) => {
                        tracing::debug!(target: "vantage_diorama::viewport", "viewport_loop: channel closed mid-debounce, exiting");
                        return;
                    }
                    Err(_) => break,
                }
            }
        }
        let Some((_, latest)) = pending.shift_remove_index(0) else {
            continue;
        };
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
        // resolves: a newer viewport from the same view for a different range
        // drops it (via `CancelOnDrop`, restoring whatever rows it had bound —
        // nothing is committed) and is served next; an identical range is
        // absorbed and the callback continues; an identical range asking for
        // a re-pull is parked and runs once this one finishes, and so does a
        // request from another view. Once the callback resolves, its commit
        // and bookkeeping (`finish_chunk_load`) run to completion outside the
        // select — a supersede can no longer reach it once the master has
        // answered.
        let load = run_chunk_callback(state.clone(), latest.clone());
        tokio::pin!(load);
        debounce_next = false;
        let finished = loop {
            tokio::select! {
                finished = &mut load => break finished,
                next = rx.recv() => match next {
                    None => {
                        tracing::debug!(target: "vantage_diorama::viewport", "viewport_loop: channel closed mid-load, exiting");
                        return;
                    }
                    Some(req) => {
                        state.viewport_queue_depth.fetch_sub(1, Ordering::SeqCst);
                        let own = req.view == latest.view;
                        if own && req.range != latest.range {
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
                            // wins next: `merge` ORs them, as the debounce
                            // coalescing does, for the same reason: dropping
                            // an `Essential` here would leave the request
                            // someone is waiting on running as a poll.
                            let key = req.view;
                            merge(&mut pending, req);
                            if let Some(at) = pending.get_index_of(&key) {
                                pending.move_index(at, 0);
                            }
                            debounce_next = true;
                            break None; // dropping `load` cancels the callback
                        }
                        // Same view, same range, already loading: absorbed,
                        // unless it asks for a re-pull.
                        if !own || req.force_load {
                            merge(&mut pending, req);
                        }
                    }
                },
            }
        };
        if let Some(finished) = finished {
            finish_chunk_load(&state, finished).await;
        }
    }
}

/// Fold `next` into the request its view already has waiting, if any: the
/// newest range, with `force_load` and `Essential` kept from either.
fn merge(pending: &mut Pending, next: ViewportRequest) {
    let Some(queued) = pending.get_mut(&next.view) else {
        pending.insert(next.view, next);
        return;
    };
    let force = queued.force_load || next.force_load;
    let essential = queued.priority.is_essential() || next.priority.is_essential();
    *queued = next;
    queued.force_load = force;
    if essential {
        queued.priority = vantage_core::Priority::Essential;
    }
}

/// Put a viewport request on the debounce channel, keeping the queue-depth
/// counter honest.
pub(crate) fn enqueue_viewport(state: &SharedRows, request: ViewportRequest) {
    // Bump the depth counter *before* sending so a racing consumer
    // can't observe a depth lower than what's actually in the channel.
    state.viewport_queue_depth.fetch_add(1, Ordering::SeqCst);
    if state.viewport_tx.send(request).is_err() {
        // Channel closed (Dio dropped). Reverse the bump so the
        // counter doesn't drift upward forever.
        state.viewport_queue_depth.fetch_sub(1, Ordering::SeqCst);
    }
}
