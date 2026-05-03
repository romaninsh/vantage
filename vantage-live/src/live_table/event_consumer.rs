//! Background task that subscribes to a `LiveStream` and invalidates the
//! cache on every event it sees. Spawned from `LiveTable::with_live_stream`.
//!
//! Sloppy invalidation: every event blows the entire `cache_key` prefix.
//! Surgical per-id invalidation is forward work — the variant is read off
//! the event for tracing only.

use std::sync::Arc;

use futures_util::StreamExt;
use tracing::{debug, instrument, warn, Instrument as _};

use crate::cache::Cache;
use crate::live_stream::{LiveEvent, LiveStream};

pub(super) fn spawn(stream: Arc<dyn LiveStream>, cache_key: String, cache: Arc<dyn Cache>) {
    // `.in_current_span()` propagates the caller's tracing span across
    // the `tokio::spawn` boundary so any tracing layer the consumer
    // installed (sentry-tracing, tracing-opentelemetry, ...) sees the
    // event-consumer's events as descendants of the caller.
    tokio::spawn(
        async move {
            let mut sub = stream.subscribe();
            debug!(target: "vantage_live::events", cache_key = %cache_key, "event consumer started");

            while let Some(event) = sub.next().await {
                handle(&cache_key, &cache, event).await;
            }
            debug!(target: "vantage_live::events", cache_key = %cache_key, "event consumer stopped");
        }
        .in_current_span(),
    );
}

#[instrument(
    target = "vantage_live::events",
    skip_all,
    fields(cache_key = %cache_key, kind = event_kind(&event))
)]
async fn handle(cache_key: &str, cache: &Arc<dyn Cache>, event: LiveEvent) {
    if let Err(e) = cache.invalidate_prefix(cache_key).await {
        warn!(
            target: "vantage_live::events",
            error = %e,
            "cache invalidation failed after live event"
        );
    } else {
        debug!(target: "vantage_live::events", outcome = "invalidated");
    }
}

fn event_kind(event: &LiveEvent) -> &'static str {
    match event {
        LiveEvent::Changed => "changed",
        LiveEvent::Inserted { .. } => "inserted",
        LiveEvent::Updated { .. } => "updated",
        LiveEvent::Deleted { .. } => "deleted",
    }
}
