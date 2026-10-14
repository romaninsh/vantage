//! Settling the grand total from a page that just landed.

use std::ops::Range;

use crate::dio::DioInner;
use crate::scenery::table::state::SharedRows;

/// Where the grand total came from, given a page that just landed. Returns
/// whether the total moved.
///
/// A total the source stated in the same response as the rows
/// (`ChunkSink::set_total`) outranks anything inferred here: a short page can
/// equally mean "the source capped the window", and reading that as the end of
/// the set would cut the grid off at its first screen. With nothing stated, a
/// short page IS the end of the set, which keeps `total` self-correcting from a
/// fetch already made — including for a list opened before its rows existed,
/// counted once at 0.
pub(super) async fn settle_total(
    state: &SharedRows,
    dio_inner: &DioInner,
    range: &Range<usize>,
    pushed: usize,
    total_before: Option<usize>,
) -> bool {
    let tap = dio_inner.tap();
    let effective_len = range.len();
    if state.take_load_total_reported() {
        let stated = *state.total.read().unwrap();
        tracing::debug!(
            target: "vantage_diorama::source",
            table = %dio_inner.master.name(),
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
            && !state.view_narrows()
            && let Some(total) = stated
            && let Err(e) = dio_inner.cache.set_meta_total(total as u64).await
        {
            tracing::debug!(
                target: "vantage_diorama::cache",
                error = %e,
                "persisting the stated total failed — the next open loses its head start",
            );
        }
        return changed;
    }

    if pushed < effective_len {
        let total = range.start + pushed;
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
        return changed;
    }

    // A FULL page from a source that has never stated a total. If it reached
    // the advertised end, that end was only ever our own inference — a
    // horizon, not a wall. Extend it by one page so the view can keep
    // scrolling and asking (the grows-as-you-scroll mode); the set's real end
    // arrives as a short page (exact) or an empty fetch (the hole clamp in
    // `clamp_to_reachable`). Without this, a total-less windowed source pinned
    // its row count at the first page and no scroll could ever request more.
    let horizon_reached = total_before.is_none_or(|t| range.end >= t);
    // Single-pass paged mode only: a two-pass view's list pass enumerated the
    // whole set — its size is knowledge, not an inference to extend.
    if state.two_pass || effective_len == 0 || !horizon_reached || state.total_ever_stated() {
        return false;
    }
    let extended = range.end + effective_len;
    tracing::debug!(
        target: "vantage_diorama::source",
        table = %dio_inner.master.name(),
        extended,
        "full page reached the inferred horizon — extending it",
    );
    crate::debug::tapline!(
        tap,
        "total",
        "{} rows — a full page reached the end we assumed; extending it",
        crate::debug::num(extended),
    );
    state.set_total_as(
        Some(extended),
        crate::scenery::table::view_stats::TotalKind::Horizon,
    )
}

/// Rows nobody can deliver. Returns whether the total moved.
///
/// A source may claim more rows than it will actually serve — a stated total
/// counting records its own paging never returns, or an offset it quietly
/// ignores so every page past a point repeats ids already held. The grid then
/// advertises slots that no fetch can fill, and since a hole is exactly what
/// triggers a fetch, it asks again, and again, for as long as the page is
/// open: one request per few seconds, forever, against the slowest thing in
/// reach.
///
/// The proof is right here and needs no cooperation from the source: this load
/// was asked for a range with holes in it and filled NONE of them. Whatever the
/// total claims, the addressable set ends at the first of those holes — so say
/// so, and the range stops being requested.
pub(super) fn clamp_to_reachable(
    state: &SharedRows,
    dio_inner: &DioInner,
    range: &Range<usize>,
    first_hole: Option<usize>,
    effective_cached: usize,
    pushed: usize,
) -> bool {
    let Some(hole) = first_hole else {
        return false;
    };
    let tap = dio_inner.tap();
    let present_after = {
        let rows = state.rows.read().unwrap();
        range.clone().filter(|i| rows.contains_key(i)).count()
    };
    if present_after != effective_cached {
        return false;
    }
    let stated = *state.total.read().unwrap();
    if stated.is_some_and(|t| t <= hole) {
        return false;
    }
    tracing::warn!(
        target: "vantage_diorama::source",
        table = %dio_inner.master.name(),
        range = ?range,
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
    state.set_total(Some(hole))
}
