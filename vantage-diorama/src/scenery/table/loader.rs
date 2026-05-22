use std::ops::Range;
use std::sync::Arc;
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
        let mut latest = initial;
        let mut absorbed = 0usize;

        // Keep absorbing requests until the channel is quiet for `debounce`.
        loop {
            match tokio::time::timeout(debounce, rx.recv()).await {
                Ok(Some(next)) => {
                    absorbed += 1;
                    latest = next;
                }
                Ok(None) => {
                    tracing::warn!(target: "vantage_diorama::viewport", "viewport_loop: channel closed mid-debounce, exiting");
                    return;
                }
                Err(_) => break,
            }
        }
        tracing::debug!(
            target: "vantage_diorama::viewport",
            range = ?latest.range,
            force_load = latest.force_load,
            absorbed,
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

    let _ = dio_inner.event_bus.send(DioEvent::ViewportChanged {
        range: visible.clone(),
    });

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
                    target: "vantage_diorama::viewport",
                    visible = ?visible,
                    visible_len,
                    visible_cached,
                    "fire_chunk_load: SKIP (visible fully cached)",
                );
                return;
            }
        }
    };

    let cb = match dio_inner.lens.callbacks.on_load_chunk.as_ref() {
        Some(cb) => cb,
        None => {
            tracing::warn!(
                target: "vantage_diorama::viewport",
                visible = ?visible,
                "fire_chunk_load: SKIP (no on_load_chunk callback)",
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
    let effective_cached = {
        let rows = state.rows.read().unwrap();
        effective_range
            .clone()
            .filter(|i| rows.contains_key(i))
            .count()
    };
    let effective_to_fetch = effective_len - effective_cached;

    let sink = ChunkSink {
        target: Arc::downgrade(&state) as std::sync::Weak<dyn crate::lens::SceneryChunkTarget>,
        cache: dio_inner.cache.clone(),
    };

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
    let result = cb(&dio, effective_range.clone(), sink).await;

    *state.load_in_flight.lock().unwrap() = None;

    let cached_after = state.rows.read().unwrap().len();
    match result {
        Ok(()) => {
            state.bump_generation();
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
    let _ = state.viewport_tx.send(request);
}
