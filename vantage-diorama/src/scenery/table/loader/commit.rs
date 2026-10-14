mod totals;

use crate::dio::DioEvent;
use crate::scenery::table::state::SharedRows;

use super::guards::{PendingChunk, restore_bound_rows};
use super::telemetry::{tap_cache_write, tap_payload_columns};
use totals::{clamp_to_reachable, settle_total};

/// Commits the rows a resolved chunk callback buffered, then runs the
/// bookkeeping that depends on the commit having happened — total
/// inference, the "rows nobody can deliver" clamp and the generation bump.
/// `viewport_loop` only calls this once `run_chunk_callback`'s future has
/// resolved on its own — never raced against the viewport channel — so none
/// of it can be cancelled.
pub(super) async fn finish_chunk_load(state: &SharedRows, pending: PendingChunk) {
    let PendingChunk {
        dio_inner,
        req,
        effective_range,
        effective_cached,
        first_hole,
        total_before,
        writer,
        mut result,
        t,
        force_load,
        priority,
        _in_flight,
    } = pending;
    let tap = dio_inner.tap();
    let effective_len = effective_range.len();

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
        Ok(()) if !state.is_chunk_loaded() => {
            // The view's query moved off windows while this load ran (to the
            // cache, or to a failure). Its rows reached the cache only, and
            // nothing of the old query's answer settles the view.
            tracing::debug!(
                target: "vantage_diorama::viewport",
                effective = ?effective_range,
                "fire_chunk_load: the view no longer serves windows; rows cached only",
            );
        }
        Ok(()) => {
            tap_cache_write(tap, state, flush_report.as_ref());
            // The window came back — whatever it contained, this view now has
            // an answer rather than a not-yet.
            state.mark_settled("chunk load succeeded");
            let pushed = state.load_push_count();
            let ms = t.elapsed().as_millis() as u64;
            // One line per fetch is for reading a session; the ledger is for
            // counting one. `already_cached` rows are the waste signal — rows
            // paid for that the cache already held.
            #[cfg(feature = "diagnostics")]
            crate::stats::record_fetch(
                dio_inner.master.name(),
                &effective_range,
                pushed,
                effective_cached,
                ms,
            );
            tracing::debug!(
                target: "vantage_diorama::source",
                table = %dio_inner.master.name(),
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
            tap_payload_columns(tap, &dio_inner, state, flush_report.as_ref());
            let mut total_changed =
                settle_total(state, &dio_inner, &effective_range, pushed, total_before).await;

            // A view serving windows has its order pushed: the window arrived
            // in the view's order, and `write_chunk_row` already placed it.
            total_changed |= clamp_to_reachable(
                state,
                &dio_inner,
                &effective_range,
                first_hole,
                effective_cached,
                pushed,
            );

            // Drain the dirty flag regardless (so it doesn't leak into the next
            // load). Bump when this was a forced refetch (a refresh or load-more —
            // it carries the single repaint for the whole refresh, including a
            // `refresh_total` that updated the count without bumping), when a
            // row changed (a refresh of byte-identical rows leaves the flag
            // clear), or when the total moved (so `row_count` consumers —
            // scrollbars, `ListDio` — repaint).
            let dirty = state.take_load_dirty();
            if force_load || dirty || total_changed {
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
            // A background load gives up on the first transient failure by
            // design and the next refresh tries again, so its failure is
            // not worth an error line; a load someone waited for is.
            if priority.is_essential() {
                tracing::error!(
                    target: "vantage_diorama::viewport",
                    effective = ?effective_range,
                    ms = t.elapsed().as_secs_f64() * 1000.0,
                    error = %e,
                    "fire_chunk_load: FAILED",
                );
            } else {
                tracing::debug!(
                    target: "vantage_diorama::viewport",
                    effective = ?effective_range,
                    ms = t.elapsed().as_secs_f64() * 1000.0,
                    error = %e,
                    "fire_chunk_load: background load gave up; the next refresh retries",
                );
            }
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
