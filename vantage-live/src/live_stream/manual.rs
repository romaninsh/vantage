//! Manually-driven `LiveStream` for tests.
//!
//! Tests that need to exercise the cache-invalidation path push events
//! into a `ManualLiveStream` and observe what happens downstream.

use futures_util::Stream;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use super::{LiveEvent, LiveStream};

/// Cheap clone, multi-subscriber, lossy on slow consumers. Backed by a
/// `tokio::sync::broadcast` channel.
#[derive(Clone)]
pub struct ManualLiveStream {
    tx: Arc<broadcast::Sender<LiveEvent>>,
}

impl ManualLiveStream {
    /// Capacity is the per-subscriber buffer; older events are dropped if a
    /// subscriber lags behind. Tests rarely want more than 16.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx: Arc::new(tx) }
    }

    /// Push an event to every current subscriber. Returns the count of
    /// subscribers that received it (zero is fine, just means nobody's
    /// listening yet).
    pub fn push(&self, event: LiveEvent) -> usize {
        self.tx.send(event).unwrap_or(0)
    }
}

impl Default for ManualLiveStream {
    fn default() -> Self {
        Self::new(16)
    }
}

impl LiveStream for ManualLiveStream {
    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = LiveEvent> + Send>> {
        let rx = self.tx.subscribe();
        // BroadcastStream yields Result<LiveEvent, BroadcastStreamRecvError>
        // — drop the error variant; it only fires on lag, which a v1
        // sloppy-invalidation consumer doesn't care about.
        Box::pin(futures_util::StreamExt::filter_map(
            BroadcastStream::new(rx),
            |r| async move { r.ok() },
        ))
    }
}
