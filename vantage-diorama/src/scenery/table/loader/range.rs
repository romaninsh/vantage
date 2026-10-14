use std::ops::Range;

use crate::dio::DioInner;
use crate::scenery::table::state::SharedRows;
use crate::scenery::table::view_stats::TotalKind;

/// The rows a viewport request fetches.
pub(super) struct Window {
    pub(super) range: Range<usize>,
    /// Whether the fetch asks past a guessed end of the set.
    pub(super) horizon_probe: bool,
    /// How many of the visible rows the map already held.
    pub(super) visible_cached: usize,
}

/// What a request for `visible` fetches, or `None` when the map already
/// holds every visible row and nothing needs asking past the end.
pub(super) fn choose_window(
    state: &SharedRows,
    dio_inner: &DioInner,
    visible: &Range<usize>,
    force_load: bool,
) -> Option<Window> {
    let total = *state.total.read().unwrap();
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
    // Only a guessed end is worth asking past. A short page proves the end
    // (`Exact`), and probing it again on every viewport costs a wasted fetch;
    // a refresh re-fetches anyway, so growth is still picked up.
    let end_is_guess = total.is_none() || *state.total_kind.read().unwrap() == TotalKind::Horizon;
    let horizon_probe = state.is_chunk_loaded()
        && !force_load
        && !state.total_ever_stated()
        && end_is_guess
        && advertised_end > 0
        && visible.end >= advertised_end;

    // Decide what to actually fetch. `force_load` callers
    // (`request_load_more`) have already pre-computed a range; respect
    // it. For viewport-driven loads, shift toward the uncached side.
    let range = if force_load {
        visible.clone()
    } else if horizon_probe {
        // Unclamped (`total: None`): the fetch may run past the inferred end.
        // Fully-cached viewport → probe one page starting AT the end; rows
        // coming back mean the set grew (or was always bigger), nothing back
        // means the inference was right and the hole clamp keeps it.
        match compute_fetch_range(state, visible, None) {
            Some(r) => r,
            None => advertised_end..advertised_end + state.page_size,
        }
    } else {
        match compute_fetch_range(state, visible, total) {
            Some(r) => r,
            None => {
                tracing::debug!(
                    target: "vantage_diorama::source",
                    table = %dio_inner.master.name(),
                    range = ?visible,
                    rows = visible_cached,
                    "CACHE — served locally, no master fetch",
                );
                let repeat = {
                    let mut last = state.last_served.lock().unwrap();
                    let same = last.as_ref() == Some(visible);
                    *last = Some(visible.clone());
                    same
                };
                if !repeat {
                    crate::debug::tapline!(
                        dio_inner.tap(),
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
    Some(Window {
        range,
        horizon_probe,
        visible_cached,
    })
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
pub(super) fn compute_fetch_range(
    state: &SharedRows,
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
