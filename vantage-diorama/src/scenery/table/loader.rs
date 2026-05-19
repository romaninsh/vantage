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
            return;
        }
        let Some(initial) = rx.recv().await else {
            return;
        };
        let mut latest = initial;

        // Keep absorbing requests until the channel is quiet for `debounce`.
        loop {
            match tokio::time::timeout(debounce, rx.recv()).await {
                Ok(Some(next)) => latest = next,
                Ok(None) => return,
                Err(_) => break,
            }
        }

        fire_chunk_load(state.clone(), latest).await;
    }
}

/// Always emits `ViewportChanged`. If the range is fully cached or no
/// `on_load_chunk` callback is registered, returns without touching
/// the cache. Otherwise dispatches the callback, then emits
/// `RangeLoaded` (success) or `LoadFailed` (error).
async fn fire_chunk_load(state: Arc<TableSceneryState>, request: ViewportRequest) {
    let ViewportRequest { range, force_load } = request;
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };

    let _ = dio_inner.event_bus.send(DioEvent::ViewportChanged {
        range: range.clone(),
    });

    if !force_load && state.range_fully_cached(&range) {
        return;
    }

    let cb = match dio_inner.lens.callbacks.on_load_chunk.as_ref() {
        Some(cb) => cb,
        None => return,
    };

    {
        let mut guard = state.load_in_flight.lock().unwrap();
        if guard.as_ref().map(|r| *r == range).unwrap_or(false) {
            return;
        }
        *guard = Some(range.clone());
    }

    let sink = ChunkSink {
        target: Arc::downgrade(&state) as std::sync::Weak<dyn crate::lens::SceneryChunkTarget>,
        cache: dio_inner.cache.clone(),
    };

    let dio = Dio {
        inner: dio_inner.clone(),
    };
    let result = cb(&dio, range.clone(), sink).await;

    *state.load_in_flight.lock().unwrap() = None;

    match result {
        Ok(()) => {
            state.bump_generation();
            let _ = dio_inner.event_bus.send(DioEvent::RangeLoaded { range });
        }
        Err(e) => {
            let _ = dio_inner.event_bus.send(DioEvent::LoadFailed {
                range,
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
