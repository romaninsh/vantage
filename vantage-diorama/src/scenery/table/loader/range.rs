use std::ops::Range;

use crate::scenery::table::state::TableSceneryState;

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
