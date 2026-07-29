use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::dio::{Dio, DioEvent};
use crate::lens::ChunkSink;

use super::ViewportRequest;
use super::state::TableSceneryState;

/// Debounces viewport requests and fires chunk loads. Restarts the
/// debounce timer on every new request — rapid scroll bursts coalesce
/// into a single fetch for the most recent range.
pub(crate) async fn viewport_loop(
    state: Arc<TableSceneryState>,
    mut rx: mpsc::UnboundedReceiver<ViewportRequest>,
    debounce: Duration,
) {
    loop {
        if state.dio_weak.upgrade().is_none() {
            tracing::warn!(target: "vantage_diorama::viewport", "viewport_loop: dio dropped, exiting");
            return;
        }
        let Some(initial) = rx.recv().await else {
            tracing::warn!(target: "vantage_diorama::viewport", "viewport_loop: channel closed, exiting");
            return;
        };
        // Decrement the producer-side counter once per pop so the
        // tracing-visible queue depth tracks what's actually pending.
        let depth_on_pop = state
            .viewport_queue_depth
            .fetch_sub(1, Ordering::SeqCst)
            .saturating_sub(1);
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
        loop {
            match tokio::time::timeout(debounce, rx.recv()).await {
                Ok(Some(next)) => {
                    state.viewport_queue_depth.fetch_sub(1, Ordering::SeqCst);
                    absorbed += 1;
                    let force = latest.force_load || next.force_load;
                    latest = next;
                    latest.force_load = force;
                }
                Ok(None) => {
                    tracing::warn!(target: "vantage_diorama::viewport", "viewport_loop: channel closed mid-debounce, exiting");
                    return;
                }
                Err(_) => break,
            }
        }
        let depth_after = state.viewport_queue_depth.load(Ordering::SeqCst);
        tracing::debug!(
            target: "vantage_diorama::viewport",
            range = ?latest.range,
            force_load = latest.force_load,
            absorbed,
            depth_on_pop,
            depth_after,
            "viewport_loop: firing",
        );
        fire_chunk_load(state.clone(), latest).await;
    }
}

/// Pick an effective fetch range given the visible viewport.
///
/// If part of `visible` is already cached, anchor the fetch at the
/// cached/uncached boundary and grow it in the *uncached* direction
/// by `page_size` rows. This eliminates the heavy overlap that happens
/// when the user drags slowly across a cached region — e.g. visible
/// `29..49` with cache `30..50` becomes a fetch of `10..30` instead of
/// re-fetching the cached portion.
///
/// Returns `None` when the visible range is fully cached.
fn compute_fetch_range(
    state: &TableSceneryState,
    visible: &Range<usize>,
    total: Option<usize>,
) -> Option<Range<usize>> {
    if visible.start >= visible.end {
        return None;
    }
    let rows = state.rows.read().unwrap();
    let page_size = state.page_size;

    let mut first_uncached: Option<usize> = None;
    let mut last_uncached: Option<usize> = None;
    let mut first_cached: Option<usize> = None;
    let mut last_cached: Option<usize> = None;
    for i in visible.clone() {
        if rows.contains_key(&i) {
            first_cached.get_or_insert(i);
            last_cached = Some(i);
        } else {
            first_uncached.get_or_insert(i);
            last_uncached = Some(i);
        }
    }
    drop(rows);

    let first_uncached = first_uncached?;
    let last_uncached = last_uncached.expect("uncached implies a last");

    let (start, end) = match (first_cached, last_cached) {
        (None, _) => {
            // Whole visible is uncached — fetch a page starting at the
            // top of the visible range so we cover it and prefetch the
            // tail in scroll direction.
            (visible.start, visible.start + page_size)
        }
        (Some(fc), Some(_)) if first_uncached < fc => {
            // Gap at the top of visible → user is scrolling up.
            // Anchor the fetch end at the first cached row and grow
            // upward by `page_size`.
            (fc.saturating_sub(page_size), fc)
        }
        (Some(_), Some(lc)) if last_uncached > lc => {
            // Gap at the bottom of visible → user is scrolling down.
            // Anchor the fetch start one past the last cached row and
            // grow downward by `page_size`.
            let s = lc + 1;
            (s, s + page_size)
        }
        _ => {
            // Hole inside visible with cache on both sides — fetch the
            // missing run plus a page in the down direction.
            (first_uncached, first_uncached + page_size)
        }
    };

    let end = match total {
        Some(t) => end.min(t),
        None => end,
    };
    if end <= start {
        return None;
    }
    Some(start..end)
}

/// Always emits `ViewportChanged`. If the range is fully cached or no
/// `on_load_chunk` callback is registered, returns without touching
/// the cache. Otherwise dispatches the callback against an
/// edge-anchored "effective" range (see [`compute_fetch_range`]), then
/// emits `RangeLoaded` (success) or `LoadFailed` (error).
async fn fire_chunk_load(state: Arc<TableSceneryState>, request: ViewportRequest) {
    let ViewportRequest {
        range: visible,
        force_load,
    } = request;
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        tracing::warn!(target: "vantage_diorama::viewport", "fire_chunk_load: dio dropped");
        return;
    };

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
    if state.two_pass {
        if !state.titles_only {
            super::two_pass::run_detail_for_range(state.clone(), visible).await;
        }
        return;
    }

    let total = *state.total.read().unwrap();
    let visible_len = visible.end.saturating_sub(visible.start);
    let visible_cached = {
        let rows = state.rows.read().unwrap();
        visible.clone().filter(|i| rows.contains_key(i)).count()
    };

    // Decide what to actually fetch. `force_load` callers
    // (`request_load_more`) have already pre-computed a range; respect
    // it. For viewport-driven loads, shift toward the uncached side.
    let effective_range = if force_load {
        visible.clone()
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
                return;
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
            return;
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
            return;
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
    };
    // The sink is moved into the callback; keep a clone so the buffered rows
    // can be committed once it returns.
    let writer = sink.clone();

    let dio = Dio {
        inner: dio_inner.clone(),
    };
    let t = std::time::Instant::now();
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
    // Clear the dirty flag so it reflects only the rows this load writes;
    // `write_chunk_row` sets it when a row's content actually changes.
    state.reset_load_dirty();
    let total_before = *state.total.read().unwrap();
    let sort = state.sort.read().unwrap().clone();
    let mut result = cb(&dio, effective_range.clone(), sort, sink).await;
    // Commit the page in one write, before anything reads the cache back. A
    // failed commit fails the load: the rows are bound in the visible map but
    // absent from the cache, and the next re-sort would rebuild the map without
    // them — better to report the load failed than to silently lose the page.
    if result.is_ok() {
        result = writer.flush().await;
    }

    *state.load_in_flight.lock().unwrap() = None;

    let cached_after = state.rows.read().unwrap().len();
    match result {
        Ok(()) => {
            // The window came back — whatever it contained, this view now has
            // an answer rather than a not-yet.
            state.mark_settled("chunk load succeeded");
            let pushed = state.load_push_count();
            let ms = t.elapsed().as_millis() as u64;
            // One line per fetch is for reading a session; the ledger is for
            // counting one. `already_cached` rows are the waste signal — rows
            // paid for that the cache already held.
            crate::stats::record_fetch(
                dio_inner.master.read().unwrap().name(),
                &effective_range,
                pushed,
                effective_cached,
                ms,
            );
            tracing::debug!(
                target: "vantage_diorama::source",
                table = %dio_inner.master.read().unwrap().name(),
                range = ?effective_range,
                received = pushed,
                ms,
                "MASTER — fetch returned",
            );
            // Where the grand total came from. A total the source stated in the
            // same response as the rows (`ChunkSink::set_total`) outranks
            // anything inferred here: a short page can equally mean "the source
            // capped the window", and reading that as the end of the set would
            // cut the grid off at its first screen. With nothing stated, a
            // short page IS the end of the set, which keeps `total`
            // self-correcting from a fetch already made — including for a list
            // opened before its rows existed, counted once at 0.
            let mut total_changed = if state.take_load_total_reported() {
                tracing::debug!(
                    target: "vantage_diorama::source",
                    table = %dio_inner.master.read().unwrap().name(),
                    total = ?*state.total.read().unwrap(),
                    "total stated by the fetch itself — no count request needed",
                );
                *state.total.read().unwrap() != total_before
            } else if pushed < effective_len {
                state.set_total(Some(effective_range.start + pushed))
            } else {
                // A FULL page from a source that has never stated a total. If
                // it reached the advertised end, that end was only ever our
                // own inference — a horizon, not a wall. Extend it by one
                // page so the view can keep scrolling and asking (the
                // grows-as-you-scroll mode); the set's real end arrives as a
                // short page (exact) or an empty fetch (the hole clamp
                // below). Without this, a total-less windowed source pinned
                // its row count at the first page and no scroll could ever
                // request more.
                let horizon_reached = total_before.is_none_or(|t| effective_range.end >= t);
                // Single-pass paged mode only: a two-pass view's list pass
                // enumerated the whole set — its size is knowledge, not an
                // inference to extend.
                if !state.two_pass && effective_len > 0 && horizon_reached && !state.total_ever_stated()
                {
                    let extended = effective_range.end + effective_len;
                    tracing::debug!(
                        target: "vantage_diorama::source",
                        table = %dio_inner.master.read().unwrap().name(),
                        extended,
                        "full page reached the inferred horizon — extending it",
                    );
                    state.set_total(Some(extended))
                } else {
                    false
                }
            };
            // A client-side sort can't push down to a paged, non-orderable
            // master, so this load (a viewport fetch, a scroll, or a refresh's
            // in-place refetch) lands rows in the master's native order. Re-impose
            // the active sort over the freshly-cached rows so the order survives
            // the refetch instead of snapping back to native order. It orders only
            // the loaded rows — the documented cost of sorting a lazily-paged
            // source that can't order server-side.
            // Only re-sort client-side when the master couldn't order it for us.
            // A `can_order` master fetched this window server-ordered (via
            // `Dio::fetch_window_ordered`), so `write_chunk_row` already stamped
            // the rows in the right order — reseeding would be redundant.
            let resorted =
                if state.sort.read().unwrap().is_some() && !state.master_capabilities.can_order {
                    if let Err(e) = state.reseed_from_cache().await {
                        tracing::error!(error = %e, "post-load resort failed");
                        false
                    } else {
                        true
                    }
                } else {
                    false
                };

            // Rows nobody can deliver.
            //
            // A source may claim more rows than it will actually serve — a
            // stated total counting records its own paging never returns, or an
            // offset it quietly ignores so every page past a point repeats ids
            // already held. The grid then advertises slots that no fetch can
            // fill, and since a hole is exactly what triggers a fetch, it asks
            // again, and again, for as long as the page is open: one request
            // per few seconds, forever, against the slowest thing in reach.
            //
            // The proof is right here and needs no cooperation from the source:
            // this load was asked for a range with holes in it and filled NONE
            // of them. Whatever the total claims, the addressable set ends at
            // the first of those holes — so say so, and the range stops being
            // requested. Measured after the resort because a client-side sort
            // rebuilds the visible map from the cache once the load lands.
            if let Some(hole) = first_hole {
                let present_after = {
                    let rows = state.rows.read().unwrap();
                    effective_range
                        .clone()
                        .filter(|i| rows.contains_key(i))
                        .count()
                };
                if present_after == effective_cached {
                    let stated = *state.total.read().unwrap();
                    if stated.map(|t| t > hole).unwrap_or(true) {
                        tracing::warn!(
                            target: "vantage_diorama::source",
                            table = %dio_inner.master.read().unwrap().name(),
                            range = ?effective_range,
                            received = pushed,
                            stated_total = ?stated,
                            reachable = hole,
                            "source served none of the requested rows — its total \
                             promises more than it delivers; capping the set at what \
                             is reachable so the fetch is not repeated forever",
                        );
                        total_changed |= state.set_total(Some(hole));
                    }
                }
            }

            // Drain the dirty flag regardless (so it doesn't leak into the next
            // load). Bump when this was a forced refetch (a refresh or load-more —
            // it carries the single repaint for the whole refresh, including a
            // `refresh_total` that updated the count without bumping), when the
            // order was re-imposed, when a row changed (a refresh of byte-identical
            // rows leaves the flag clear), or when the total moved (so `row_count`
            // consumers — scrollbars, `ListDio` — repaint).
            let dirty = state.take_load_dirty();
            if force_load || resorted || dirty || total_changed {
                state.bump_generation();
            }
            tracing::debug!(
                target: "vantage_diorama::viewport",
                effective = ?effective_range,
                effective_len,
                ms = t.elapsed().as_secs_f64() * 1000.0,
                cached_after,
                "fire_chunk_load: OK",
            );
            let _ = dio_inner.event_bus.send(DioEvent::RangeLoaded {
                range: effective_range,
            });
        }
        Err(e) => {
            tracing::error!(
                target: "vantage_diorama::viewport",
                effective = ?effective_range,
                ms = t.elapsed().as_secs_f64() * 1000.0,
                error = %e,
                "fire_chunk_load: FAILED",
            );
            let _ = dio_inner.event_bus.send(DioEvent::LoadFailed {
                range: effective_range,
                error: e.to_string(),
            });
        }
    }
}

/// Convenience wrapper used by `set_viewport` and `request_load_more`
/// to enqueue a viewport request on the debounce channel.
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
