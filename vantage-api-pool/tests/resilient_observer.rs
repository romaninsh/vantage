//! The observer contract: one terminal event per `Started`, breaker
//! transitions in order, and `ms` that measures only the request.

mod support;

use std::sync::Arc;
use std::time::Duration;

use support::{
    background_error, counting_refresher, essential_fast, mount_script, mount_status, requests,
    scripted_refresher, within, Recorder,
};
use vantage_api_pool::resilient::{ErrorKind, TransportEvent};
use vantage_api_pool::ResilientClient;
use wiremock::MockServer;

#[tokio::test]
async fn observer_sees_every_attempt_retry_and_breaker_transition() {
    let server = MockServer::start().await;
    mount_script(&server, [503, 503, 200]).await;
    let rec = Arc::new(Recorder::default());
    let client = ResilientClient::builder()
        .observer("local", rec.clone())
        .circuit_breaker_growing(2, Duration::from_millis(1), Duration::from_millis(1))
        .build();
    let url = server.uri();
    within(client.execute_with(&essential_fast(), |h| h.get(&url)))
        .await
        .expect("third attempt succeeds");
    client.report(TransportEvent::RowsPulled { n: 7 });

    assert_eq!(rec.key().as_deref(), Some("local"));
    assert_eq!(client.key(), Some("local"));
    assert_eq!(
        rec.tags(),
        [
            "started",
            "failed:status",
            "retry:1",
            "started",
            "failed:status",
            "opened",
            "retry:2",
            "started",
            "ok:200",
            "closed",
            "rows:7",
        ]
    );
}

#[tokio::test]
async fn observer_pairs_the_401_attempt_with_a_failed_event() {
    let server = MockServer::start().await;
    mount_script(&server, [401, 200]).await;

    let (refresher, calls) = counting_refresher();
    let rec = Arc::new(Recorder::default());
    let client = ResilientClient::builder()
        .observer("local", rec.clone())
        .bearer_auth(refresher)
        .build();
    let url = server.uri();
    client
        .execute(|h| h.get(&url))
        .await
        .expect("replays after the refresh");

    assert_eq!(
        rec.tags(),
        ["started", "failed:status", "started", "ok:200"]
    );
    assert_eq!(
        calls.count(),
        2,
        "one lazy acquire plus one refresh on the 401"
    );
}

#[tokio::test]
async fn a_failing_refresher_reports_a_failed_event() {
    let server = MockServer::start().await;
    mount_status(&server, 200).await;

    let (refresher, _) = scripted_refresher(|_| Err(anyhow::anyhow!("token endpoint down")));
    let rec = Arc::new(Recorder::default());
    let client = ResilientClient::builder()
        .observer("local", rec.clone())
        .bearer_auth(refresher)
        .build();
    let url = server.uri();
    let err = client.execute(|h| h.get(&url)).await.unwrap_err();

    assert_eq!(err.kind_name(), "auth");
    assert_eq!(
        rec.tags(),
        ["started", "failed:auth"],
        "the attempt's Started must not be left without a terminal event"
    );
}

#[tokio::test]
async fn a_failing_reacquire_reports_its_own_started() {
    let server = MockServer::start().await;
    mount_status(&server, 401).await;

    // The lazy acquire works; the refresh triggered by the 401 does not.
    let (refresher, _) = scripted_refresher(|n| match n {
        0 => Ok("token-0".to_string()),
        _ => Err(anyhow::anyhow!("token endpoint down")),
    });
    let rec = Arc::new(Recorder::default());
    let client = ResilientClient::builder()
        .observer("local", rec.clone())
        .bearer_auth(refresher)
        .build();
    let url = server.uri();
    let err = client.execute(|h| h.get(&url)).await.unwrap_err();

    assert_eq!(err.kind_name(), "auth");
    assert_eq!(
        rec.tags(),
        ["started", "failed:status", "started", "failed:auth"],
        "the replay that never ran still gets a paired Started/Failed"
    );
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test]
async fn observer_reports_fail_fast_and_probe_closing_on_404() {
    let server = MockServer::start().await;
    mount_script(&server, [503, 404]).await;

    let rec = Arc::new(Recorder::default());
    let client = ResilientClient::builder()
        .observer("local", rec.clone())
        .circuit_breaker_growing(1, Duration::from_millis(20), Duration::from_millis(20))
        .build();
    let url = server.uri();

    // Call 1: the 503 opens the breaker (threshold 1).
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::Status(503)
    );
    // Call 2: fails fast on the still-open breaker.
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::BreakerOpen
    );
    tokio::time::sleep(Duration::from_millis(25)).await;
    // Call 3: the probe gets a 404, which closes the breaker.
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::Status(404)
    );

    assert_eq!(
        rec.tags(),
        [
            "started",
            "failed:status",
            "opened",
            "started",
            "failed:breaker_open",
            "started",
            "failed:status",
            "closed",
        ]
    );
    assert_eq!(
        rec.failed_ms()[1],
        0,
        "the fail-fast Failed carries ms: 0 and never touched the network"
    );
}

#[tokio::test]
async fn observer_ms_excludes_the_breaker_wait() {
    let server = MockServer::start().await;
    mount_script(&server, [503, 200]).await;

    let rec = Arc::new(Recorder::default());
    let client = ResilientClient::builder()
        .observer("local", rec.clone())
        .circuit_breaker_growing(1, Duration::from_millis(200), Duration::from_millis(200))
        .build();
    let url = server.uri();

    // Call 1: the 503 opens the breaker (threshold 1).
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::Status(503)
    );
    // Call 2: waits out the 200 ms cooldown, takes the probe, succeeds.
    within(client.execute_with(&essential_fast(), |h| h.get(&url)))
        .await
        .expect("waits for the probe and succeeds");

    assert_eq!(
        rec.tags(),
        [
            "started",
            "failed:status",
            "opened",
            "started",
            "ok:200",
            "closed"
        ]
    );
    let ms = rec.succeeded_ms()[0];
    assert!(
        ms < 100,
        "ms must exclude the 200 ms breaker wait, was {ms}"
    );
}
