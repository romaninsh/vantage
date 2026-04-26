//! Test 4: external `LiveStream` events invalidate the cache.

mod common;

use std::sync::Arc;
use std::time::Duration;

use vantage_dataset::traits::ReadableValueSet;
use vantage_live::cache::{Cache, MemCache};
use vantage_live::{LiveEvent, LiveTable, ManualLiveStream};

/// Poll an async predicate every few ms until it returns true or the
/// timeout elapses. Bridges the gap between "event pushed" and "consumer
/// task observed it" without baking in a fixed sleep.
async fn await_async<F, Fut>(timeout_ms: u64, mut pred: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if pred().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    false
}

#[tokio::test]
async fn changed_event_invalidates_cache() {
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let cache = MemCache::new();
    let stream = ManualLiveStream::default();

    let live = LiveTable::new(master, "products", Arc::new(cache.clone()))
        .with_live_stream(Arc::new(stream.clone()));

    // Warm the cache, then push an event.
    let _ = live.list_values().await.unwrap();
    let key = live.page_cache_key(1);
    assert!(cache.get(&key).await.unwrap().is_some());

    // Yield once so the consumer task gets a chance to start subscribing
    // before we push (broadcast::send to a channel with no subscribers
    // returns 0 — the event would be lost).
    tokio::task::yield_now().await;
    let n = stream.push(LiveEvent::Changed);
    assert!(n >= 1, "consumer should be subscribed by now");

    let cache_for_check = cache.clone();
    let key_for_check = key.clone();
    let invalidated = await_async(500, move || {
        let cache = cache_for_check.clone();
        let key = key_for_check.clone();
        async move { cache.get(&key).await.unwrap().is_none() }
    })
    .await;

    assert!(
        invalidated,
        "cache should have been invalidated by the live event"
    );
}

#[tokio::test]
async fn id_specific_event_also_invalidates_for_v1() {
    // v1 invalidates the whole cache_key on every event regardless of
    // variant. This test pins that behaviour.
    let (_tmp, master, _typed) = common::seeded_redb_master("products").await;
    let cache = MemCache::new();
    let stream = ManualLiveStream::default();

    let live = LiveTable::new(master, "products", Arc::new(cache.clone()))
        .with_live_stream(Arc::new(stream.clone()));

    let _ = live.list_values().await.unwrap();
    let _ = live.get_value(&"a".to_string()).await.unwrap();

    let page_key = live.page_cache_key(1);
    let id_key = live.id_cache_key("a");
    assert!(cache.get(&page_key).await.unwrap().is_some());
    assert!(cache.get(&id_key).await.unwrap().is_some());

    tokio::task::yield_now().await;
    stream.push(LiveEvent::Updated { id: "a".into() });

    let cache_for_check = cache.clone();
    let invalidated = await_async(500, move || {
        let cache = cache_for_check.clone();
        let page_key = page_key.clone();
        let id_key = id_key.clone();
        async move {
            cache.get(&page_key).await.unwrap().is_none()
                && cache.get(&id_key).await.unwrap().is_none()
        }
    })
    .await;

    assert!(invalidated, "both page and id slots should be invalidated");
}
