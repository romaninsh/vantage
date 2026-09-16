use std::sync::Arc;

use crate::dio::{Dio, DioEvent};
use crate::lens::ChunkSink;
use crate::scenery::table::ViewportRequest;
use crate::scenery::table::state::{InFlightMarker, TableSceneryState};

use super::guards::{CancelOnDrop, PendingChunk};
use super::range::compute_fetch_range;

/// Always emits `ViewportChanged`. If the range is fully cached or no
/// `on_load_chunk` callback is registered, returns `None` without touching
/// the cache. Otherwise dispatches the callback against an edge-anchored
/// "effective" range (see [`compute_fetch_range`]) and, once it resolves,
/// returns `Some(PendingChunk)` for
/// [`finish_chunk_load`](super::commit::finish_chunk_load) to commit — a
/// `select!` racing this function against the viewport channel can cancel
/// it (by dropping its future) any time before that `Some` comes back, and
/// `CancelOnDrop` restores whatever rows the callback had bound so a
/// cancelled load leaves the visible map as it found it.
pub(super) async fn run_chunk_callback(
    state: Arc<TableSceneryState>,
    request: ViewportRequest,
) -> Option<PendingChunk> {
    let ViewportRequest {
        range: visible,
        force_load,
        priority,
    } = request;
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        tracing::warn!(target: "vantage_diorama::viewport", "fire_chunk_load: dio dropped");
        return None;
    };
    let tap = dio_inner.tap();

    // Remember the window the grid last asked for so a refresh can re-fetch
    // exactly it in place (see `TableSceneryState::refresh_loaded_viewport`).
    *state.last_viewport.write().unwrap() = Some(visible.clone());

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
    if state.two_pass {
        if !state.titles_only {
            crate::scenery::table::two_pass::run_detail_for_range(state.clone(), visible).await;
        }
        return None;
    }

    let total = *state.total.read().unwrap();
    let visible_len = visible.end.saturating_sub(visible.start);
    let visible_cached = {
        let rows = state.rows.read().unwrap();
        visible.clone().filter(|i| rows.contains_key(i)).count()
    };

    // Unknown-total horizon probe. When the source has never stated a total,
    // the advertised end is only our inference — and a viewport pressed
    // against it must ASK past it, even when every visible row is cached.
    // Without this, a warm cache walls the set at its seeded size forever:
    // the visible range is fully cached, no fetch fires, and the fetch is
    // the only thing that can extend the horizon.
    // What the view believes the set's end is: the inferred total, or — on a
    // cache-seeded reopen, where no total survived — the seeded row count.
    let advertised_end =
        total.unwrap_or_else(|| state.rows.read().unwrap().keys().max().map_or(0, |i| i + 1));
    let horizon_probe = state.paged
        && !state.two_pass
        && !force_load
        && !state.total_ever_stated()
        && advertised_end > 0
        && visible.end >= advertised_end;

    // Decide what to actually fetch. `force_load` callers
    // (`request_load_more`) have already pre-computed a range; respect
    // it. For viewport-driven loads, shift toward the uncached side.
    let effective_range = if force_load {
        visible.clone()
    } else if horizon_probe {
        // Unclamped (`total: None`): the fetch may run past the inferred end.
        // Fully-cached viewport → probe one page starting AT the end; rows
        // coming back mean the set grew (or was always bigger), nothing back
        // means the inference was right and the hole clamp keeps it.
        match compute_fetch_range(&state, &visible, None) {
            Some(r) => r,
            None => advertised_end..advertised_end + state.page_size,
        }
    } else {
        match compute_fetch_range(&state, &visible, total) {
            Some(r) => r,
            None => {
                tracing::debug!(
                    target: "vantage_diorama::source",
                    table = %dio_inner.master.read().unwrap().name(),
                    range = ?visible,
                    rows = visible_cached,
                    "CACHE — served locally, no master fetch",
                );
                let repeat = {
                    let mut last = state.last_served.lock().unwrap();
                    let same = last.as_ref() == Some(&visible);
                    *last = Some(visible.clone());
                    same
                };
                if !repeat {
                    crate::debug::tapline!(
                        tap,
                        "cache",
                        "all {} rows of {}..{} served locally — no fetch",
                        visible_cached,
                        visible.start,
                        visible.end,
                    );
                }
                return None;
            }
        }
    };

    // An eager lens holds every row already, so it registers no
    // `on_load_chunk` — a viewport it can't page for is its steady state,
    // not a fault. Logged at DEBUG like the fully-cached skip above:
    // anything that re-drives the viewport on a timer (a relation list's
    // periodic re-pull, `refresh_loaded_viewport`) would otherwise emit a
    // warning per tick, forever.
    // Only a paged view fetches windows. An eager one holds every row already
    // — its viewport moving is its steady state, not a reason to fetch — and
    // its master may not be able to serve a window at all, which is an error
    // raised to the user rather than a quiet no-op.
    let cb = match dio_inner
        .lens
        .callbacks
        .on_load_chunk
        .as_ref()
        .filter(|_| state.paged)
    {
        Some(cb) => cb,
        None => {
            tracing::debug!(
                target: "vantage_diorama::viewport",
                visible = ?visible,
                paged = state.paged,
                "fire_chunk_load: SKIP (this view does not page)",
            );
            return None;
        }
    };

    {
        let mut guard = state.load_in_flight.lock().unwrap();
        if guard
            .as_ref()
            .map(|r| *r == effective_range)
            .unwrap_or(false)
        {
            tracing::debug!(
                target: "vantage_diorama::viewport",
                effective = ?effective_range,
                "fire_chunk_load: SKIP (same range already in flight)",
            );
            return None;
        }
        if let Some(prev) = guard.as_ref() {
            tracing::warn!(
                target: "vantage_diorama::viewport",
                prev = ?prev,
                effective = ?effective_range,
                "fire_chunk_load: overwriting in-flight marker (viewport_loop is supposed to be serial)",
            );
        }
        *guard = Some(effective_range.clone());
    }
    let in_flight = InFlightMarker(state.clone());

    // Only allocate a request id when the tap is enabled — it's the one
    // correlator that ties this fetch's "load dispatch" to its "load return"
    // / "load failed" in the debug stream, and paying for it off the tap is
    // pointless.
    let req = tap.enabled().then(|| dio_inner.next_req());

    // Recompute overlap on the effective range so the log shows the
    // shift working.
    let effective_len = effective_range.end - effective_range.start;
    let (effective_cached, first_hole) = {
        let rows = state.rows.read().unwrap();
        let present = effective_range
            .clone()
            .filter(|i| rows.contains_key(i))
            .count();
        let first_hole = effective_range.clone().find(|i| !rows.contains_key(i));
        (present, first_hole)
    };
    let effective_to_fetch = effective_len - effective_cached;

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

    let dio = Dio {
        inner: dio_inner.clone(),
    };
    let t = std::time::Instant::now();
    // Built here (rather than beside the `cb` call below) so the "load
    // dispatch" tapline can report the query the fetch is about to run —
    // sort/search included.
    let query = crate::lens::ChunkQuery {
        sort: state.sort.read().unwrap().clone(),
        search: state.search.read().unwrap().clone(),
        filters: state.ui_terms.read().unwrap().clone(),
    };
    tracing::debug!(
        target: "vantage_diorama::viewport",
        visible = ?visible,
        visible_len,
        visible_cached,
        effective = ?effective_range,
        effective_len,
        effective_cached,
        effective_to_fetch,
        effective_overfetch_pct = if effective_len > 0 {
            (effective_cached as f64 / effective_len as f64) * 100.0
        } else {
            0.0
        },
        force_load,
        priority = ?priority,
        "fire_chunk_load: dispatching on_load_chunk",
    );
    // The counterpart of the CACHE line above: this is the moment a range
    // costs a round trip. `already_cached` says how much of what we are about
    // to fetch the cache ALREADY holds — non-zero means the fetch is
    // re-reading rows we have, which on a `force_load` is by design and
    // otherwise is worth explaining.
    tracing::debug!(
        target: "vantage_diorama::source",
        table = %dio_inner.master.read().unwrap().name(),
        range = ?effective_range,
        rows = effective_to_fetch,
        already_cached = effective_cached,
        force_load,
        "MASTER — fetching from the datasource",
    );
    crate::debug::tapline!(
        tap,
        "dio",
        "fetch #{} asks for rows {}..{}{} — {} missing, {} already held{}{}",
        req.unwrap_or_default(),
        effective_range.start,
        effective_range.end,
        if effective_range == visible {
            String::new()
        } else {
            format!(" (viewport {}..{})", visible.start, visible.end)
        },
        effective_to_fetch,
        effective_cached,
        match &query.sort {
            Some((col, dir)) => format!(" · sorted by {col} {dir:?}"),
            None => String::new(),
        },
        match &query.search {
            Some(q) => format!(" · searching \"{q}\""),
            None => String::new(),
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
        .scope(cb(&dio, effective_range.clone(), query, sink))
        .await;
    cancel_guard.armed = false;

    Some(PendingChunk {
        dio_inner,
        req,
        effective_range,
        effective_len,
        effective_cached,
        first_hole,
        total_before,
        writer,
        result,
        t,
        force_load,
        _in_flight: in_flight,
    })
}
