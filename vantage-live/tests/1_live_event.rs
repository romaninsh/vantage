//! Test 1: LiveEvent variants and ManualLiveStream wiring.
//!
//! Confirms the trait shape: subscribers receive pushed events, multiple
//! subscribers each get their own copy, and a stream survives an empty
//! period without errors.

use futures_util::StreamExt;
use vantage_live::live_stream::{LiveEvent, LiveStream, ManualLiveStream};

#[tokio::test]
async fn manual_stream_pushes_event_to_subscriber() {
    let stream = ManualLiveStream::default();
    let mut sub = stream.subscribe();

    stream.push(LiveEvent::Changed);
    let received = sub.next().await.expect("event arrives");
    assert_eq!(received, LiveEvent::Changed);
}

#[tokio::test]
async fn manual_stream_carries_id_variants() {
    let stream = ManualLiveStream::default();
    let mut sub = stream.subscribe();

    stream.push(LiveEvent::Inserted { id: "marty".into() });
    stream.push(LiveEvent::Updated { id: "doc".into() });
    stream.push(LiveEvent::Deleted { id: "biff".into() });

    assert_eq!(
        sub.next().await,
        Some(LiveEvent::Inserted { id: "marty".into() })
    );
    assert_eq!(
        sub.next().await,
        Some(LiveEvent::Updated { id: "doc".into() })
    );
    assert_eq!(
        sub.next().await,
        Some(LiveEvent::Deleted { id: "biff".into() })
    );
}

#[tokio::test]
async fn manual_stream_multi_subscriber_each_sees_event() {
    let stream = ManualLiveStream::default();
    let mut sub_a = stream.subscribe();
    let mut sub_b = stream.subscribe();

    let n = stream.push(LiveEvent::Changed);
    assert_eq!(n, 2);

    assert_eq!(sub_a.next().await, Some(LiveEvent::Changed));
    assert_eq!(sub_b.next().await, Some(LiveEvent::Changed));
}

#[tokio::test]
async fn manual_stream_push_with_no_subscribers_is_zero() {
    let stream = ManualLiveStream::default();
    // No subscribers yet — push returns 0, doesn't panic.
    let n = stream.push(LiveEvent::Changed);
    assert_eq!(n, 0);
}

#[tokio::test]
async fn live_stream_can_be_used_as_trait_object() {
    use std::sync::Arc;
    let stream: Arc<dyn LiveStream> = Arc::new(ManualLiveStream::default());
    let mut sub = stream.subscribe();

    // We need a concrete handle to push, so we construct one alongside.
    // (Actual production wiring keeps the concrete handle on the
    // implementor's side.)
    let pusher = ManualLiveStream::default();
    let mut sub2 = pusher.subscribe();
    pusher.push(LiveEvent::Changed);

    // The dyn-cast subscription is its own stream (different ManualLiveStream);
    // assert it's at least pollable without blocking on a missing event.
    use std::time::Duration;
    let res = tokio::time::timeout(Duration::from_millis(20), sub.next()).await;
    assert!(res.is_err(), "should time out, no event was pushed to it");

    // The other one we did push to receives the event.
    assert_eq!(sub2.next().await, Some(LiveEvent::Changed));
}
