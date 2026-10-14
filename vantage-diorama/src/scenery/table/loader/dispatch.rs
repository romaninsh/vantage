use std::sync::Arc;

use crate::dio::{Dio, DioEvent};
use crate::lens::{ChunkSink, LoaderCx};
use crate::scenery::table::ViewportRequest;
use crate::scenery::table::state::SharedRows;

use super::guards::{CancelOnDrop, PendingChunk, claim_in_flight};
use super::range::{Window, choose_window};
use super::telemetry::{Dispatch, trace_dispatch};

/// Always emits `ViewportChanged`. If the range is fully cached or the view
/// doesn't serve its query from windows, returns `None` without touching
/// the cache. Otherwise dispatches the callback against an edge-anchored
/// "effective" range (see [`choose_window`]) and, once it resolves,
/// returns `Some(PendingChunk)` for
/// [`finish_chunk_load`](super::commit::finish_chunk_load) to commit — a
/// `select!` racing this function against the viewport channel can cancel
/// it (by dropping its future) any time before that `Some` comes back, and
/// `CancelOnDrop` restores whatever rows the callback had bound so a
/// cancelled load leaves the visible map as it found it.
pub(super) async fn run_chunk_callback(
    state: Arc<SharedRows>,
    request: ViewportRequest,
) -> Option<PendingChunk> {
    let ViewportRequest {
        range: visible,
        force_load,
        priority,
        view,
    } = request;
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        tracing::warn!(target: "vantage_diorama::viewport", "fire_chunk_load: dio dropped");
        return None;
    };
    let tap = dio_inner.tap();

    // Remember the window the view last asked for so a refresh can re-fetch
    // exactly it in place (see `SharedRows::refresh_loaded_viewport`).
    if let Some(view) = view {
        state.record_viewport(view, visible.clone());
    }

    let _ = dio_inner.event_bus.send(DioEvent::ViewportChanged {
        range: visible.clone(),
    });

    // Two-pass: a viewport drives the detail pass over the visible rows, not a
    // random-access chunk load. Hydration dedups against already-`Complete`
    // ids, so re-entering the same viewport is a no-op.
    //
    // A `titles_only` picker skips hydration entirely — it only needs the
    // cheap list columns, so the viewport just commits (the `ViewportChanged`
    // above) and no detail fetch is issued.
    //
    // The sweep runs here, inside the cancellable phase, so a viewport that
    // moves on drops it mid-flight. That is safe because it self-heals: the
    // superseding viewport sweeps its own rows, and hydration dedups against
    // ids already `Complete`, so a half-finished sweep costs at most a repeat
    // of the rows it did not reach.
    //
    // The rows hydrate under the ticket of the view that asked, so two views
    // on the same rows keep their own places in the scheduler's round robin.
    if state.two_pass {
        if !state.titles_only {
            let ticket = state.ticket_for(view);
            crate::scenery::table::two_pass::run_detail_for_range(state.clone(), visible, ticket)
                .await;
        }
        return None;
    }

    let Window {
        range: effective_range,
        horizon_probe,
        visible_cached,
    } = choose_window(&state, &dio_inner, &visible, force_load)?;

    // Only a view serving windows fetches them: one whose query the master
    // takes whole, so it has a plan with no residual. An eager view holds
    // every row already, so its viewport moving is its steady state, not a
    // fault; a paged view whose query leaves a residual is answered from the
    // cache or has failed, and a window would answer neither. Logged at
    // DEBUG like the fully-cached skip in `choose_window`: anything that
    // re-drives the viewport on a timer (a relation list's periodic re-pull,
    // `refresh_loaded_viewport`) would otherwise emit a line per tick,
    // forever.
    let (Some(cb), Some(plan)) = (
        dio_inner.lens.callbacks.loader.paged.as_ref(),
        state.view_plan().filter(|_| state.is_chunk_loaded()),
    ) else {
        tracing::debug!(
            target: "vantage_diorama::viewport",
            visible = ?visible,
            paged = state.paged,
            "fire_chunk_load: SKIP (this view does not serve its query from windows)",
        );
        return None;
    };

    let in_flight = claim_in_flight(&state, &effective_range)?;

    // Only allocate a request id when the tap is enabled — it's the one
    // correlator that ties this fetch's "load dispatch" to its "load return"
    // / "load failed" in the debug stream, and paying for it off the tap is
    // pointless.
    let req = tap.enabled().then(|| dio_inner.next_req());

    // Recompute overlap on the effective range so the log shows the
    // shift working.
    let (effective_cached, first_hole) = {
        let rows = state.rows.read().unwrap();
        let present = effective_range
            .clone()
            .filter(|i| rows.contains_key(i))
            .count();
        let first_hole = effective_range.clone().find(|i| !rows.contains_key(i));
        (present, first_hole)
    };

    let sink = ChunkSink {
        target: Arc::downgrade(&state) as std::sync::Weak<dyn crate::lens::SceneryChunkTarget>,
        cache: dio_inner.cache.clone(),
        pending: dio_inner.pending_flashes.clone(),
        buffer: Default::default(),
        debug: tap.enabled(),
    };
    // The sink is moved into the callback; keep a clone so the buffered rows
    // can be committed once it returns.
    let writer = sink.clone();

    let cx = LoaderCx::new(Dio {
        inner: dio_inner.clone(),
    });
    let t = std::time::Instant::now();
    // A view serving windows has a plan with no residual: the master takes
    // this whole query.
    let query = plan.query().clone();
    trace_dispatch(
        tap,
        &dio_inner,
        Dispatch {
            req,
            visible: &visible,
            visible_cached,
            effective: &effective_range,
            effective_cached,
            force_load,
            priority,
            query: &query,
        },
    );
    // Clear the dirty flag so it reflects only the rows this load writes;
    // `write_chunk_row` sets it when a row's content actually changes.
    state.reset_load_dirty();
    let total_before = *state.total.read().unwrap();
    // The horizon probe asks past the inferred end on the user's behalf but
    // nobody is waiting on it; everything else runs under the priority the
    // producer declared. The scope is here, inside the viewport task, so it
    // reaches the transport; spawning the callback would lose it.
    let priority = if horizon_probe {
        vantage_core::Priority::Background
    } else {
        priority
    };
    // Armed for the duration of the callback: if `select!` in `viewport_loop`
    // drops this future here (a newer viewport superseded it), the guard
    // restores whatever rows the callback had bound before it finished.
    let mut cancel_guard = CancelOnDrop {
        armed: true,
        writer: writer.clone(),
    };
    let result = priority
        .scope(cb(cx, effective_range.clone(), query, sink))
        .await;
    cancel_guard.armed = false;

    Some(PendingChunk {
        dio_inner,
        req,
        effective_range,
        effective_cached,
        first_hole,
        total_before,
        writer,
        result,
        t,
        force_load,
        priority,
        _in_flight: in_flight,
    })
}
