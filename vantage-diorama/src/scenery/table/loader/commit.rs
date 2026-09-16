use std::sync::Arc;

use crate::dio::DioEvent;
use crate::scenery::table::state::TableSceneryState;

use super::guards::{PendingChunk, restore_bound_rows};
use super::telemetry::{tap_cache_write, tap_payload_columns};

/// Commits the rows a resolved chunk callback buffered, then runs the
/// bookkeeping that depends on the commit having happened — total
/// inference, the client-side resort, the "rows nobody can deliver" clamp,
/// and the generation bump. `viewport_loop` only calls this once
/// `run_chunk_callback`'s future has resolved on its own — never raced
/// against the viewport channel — so none of it can be cancelled.
pub(super) async fn finish_chunk_load(state: Arc<TableSceneryState>, pending: PendingChunk) {
    let PendingChunk {
        dio_inner,
        req,
        effective_range,
        effective_len,
        effective_cached,
        first_hole,
        total_before,
        writer,
        mut result,
        t,
        force_load,
        _in_flight,
    } = pending;
    let tap = dio_inner.tap();

    // Commit the page in one write, before anything reads the cache back. A
    // failed commit fails the load: the rows are bound in the visible map but
    // absent from the cache, and the next re-sort would rebuild the map without
    // them — better to report the load failed than to silently lose the page.
    // `flush_counted` also carries the before/after cache row counts the
    // "cache write" debug line reports — free of extra cost when the tap is
    // off, since the sink itself skips the counting in that case.
    let flush_report = if result.is_ok() {
        match writer.flush_counted().await {
            Ok(report) => Some(report),
            Err(e) => {
                result = Err(e);
                None
            }
        }
    } else {
        None
    };

    let cached_after = state.rows.read().unwrap().len();
    match result {
        Ok(()) => {
            // `flush_report` is always `Some` here — flush only fails when
            // `result` does, which routes to the `Err` arm.
            if let Some(report) = &flush_report {
                tap_cache_write(tap, &state, report);
            }
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
            crate::debug::tapline!(
                tap,
                "dio",
                "fetch #{} got {} rows in {}{}",
                req.unwrap_or_default(),
                pushed,
                crate::debug::dur(ms),
                if ms >= 1_000 { "  ⚠ slow" } else { "" },
            );
            tap_payload_columns(tap, &dio_inner, &state, flush_report.as_ref());
            // Where the grand total came from. A total the source stated in the
            // same response as the rows (`ChunkSink::set_total`) outranks
            // anything inferred here: a short page can equally mean "the source
            // capped the window", and reading that as the end of the set would
            // cut the grid off at its first screen. With nothing stated, a
            // short page IS the end of the set, which keeps `total`
            // self-correcting from a fetch already made — including for a list
            // opened before its rows existed, counted once at 0.
            let mut total_changed = if state.take_load_total_reported() {
                let stated = *state.total.read().unwrap();
                tracing::debug!(
                    target: "vantage_diorama::source",
                    table = %dio_inner.master.read().unwrap().name(),
                    total = ?stated,
                    "total stated by the fetch itself — no count request needed",
                );
                let changed = stated != total_before;
                // Only when it MOVES. A source that restates the same total on
                // every window would otherwise repeat this line per fetch,
                // burying the times it actually changed.
                if changed {
                    crate::debug::tapline!(
                        tap,
                        "total",
                        "{} rows — stated by the source in the same response",
                        crate::debug::num(stated.unwrap_or_default()),
                    );
                }
                // Remember a stated, un-narrowed total in the cache meta, so
                // the NEXT open of this view can size its geometry before any
                // fetch — the difference between a warm reopen appearing
                // whole and its row count visibly jumping when the first
                // counted response lands. A total under an active search or
                // filter describes the narrowed set and must not be
                // remembered — the next open would restore it as the whole.
                if changed
                    && state.search.read().unwrap().is_none()
                    && state.ui_terms.read().unwrap().is_empty()
                    && let Some(total) = stated
                    && let Err(e) = dio_inner.cache.set_meta_total(total as u64).await
                {
                    tracing::debug!(
                        target: "vantage_diorama::cache",
                        error = %e,
                        "persisting the stated total failed — the next open loses its head start",
                    );
                }
                changed
            } else if pushed < effective_len {
                let total = effective_range.start + pushed;
                // Same rule as the stated branch above: only when it MOVES. A
                // reload that keeps landing on the same short page would
                // otherwise repeat this line for a total that never changed.
                let changed = state.set_total(Some(total));
                if changed {
                    crate::debug::tapline!(
                        tap,
                        "total",
                        "{} rows — inferred: the page came back short",
                        crate::debug::num(total),
                    );
                }
                changed
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
                if !state.two_pass
                    && effective_len > 0
                    && horizon_reached
                    && !state.total_ever_stated()
                {
                    let extended = effective_range.end + effective_len;
                    tracing::debug!(
                        target: "vantage_diorama::source",
                        table = %dio_inner.master.read().unwrap().name(),
                        extended,
                        "full page reached the inferred horizon — extending it",
                    );
                    crate::debug::tapline!(
                        tap,
                        "total",
                        "{} rows — a full page reached the end we assumed; extending it",
                        crate::debug::num(extended),
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
                        crate::debug::tapline!(
                            tap,
                            "total",
                            "{} rows — capped: the source promises more than it serves",
                            crate::debug::num(hole),
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
            state.note_state("chunk load");
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
            // The callback pushed rows and then failed (or the flush itself
            // failed), so those rows are bound in the visible map with nothing
            // behind them in the cache. That is the same inconsistency a
            // cancelled load leaves, so it unwinds the same way: each bound
            // slot goes back to the record it held, and a failed refresh is
            // once again invisible to the grid.
            restore_bound_rows(&writer);
            tracing::error!(
                target: "vantage_diorama::viewport",
                effective = ?effective_range,
                ms = t.elapsed().as_secs_f64() * 1000.0,
                error = %e,
                "fire_chunk_load: FAILED",
            );
            crate::debug::tapline!(
                tap,
                "dio",
                "fetch #{} failed after {} — {}",
                req.unwrap_or_default(),
                crate::debug::dur(t.elapsed().as_millis() as u64),
                e,
            );
            let _ = dio_inner.event_bus.send(DioEvent::LoadFailed {
                range: effective_range,
                error: e.to_string(),
            });
        }
    }
}
