//! The per-client token bucket: it spaces attempts out, and the waiting for a
//! token is never charged to the request.

mod support;

use std::sync::Arc;
use std::time::Duration;

use support::{mount_status, requests, Recorder};
use vantage_api_pool::ResilientClient;
use wiremock::MockServer;

#[tokio::test]
async fn rate_limit_spaces_requests_out() {
    let server = MockServer::start().await;
    mount_status(&server, 200).await;
    // 100/s → 10 ms apart. `rate_limit(100.0)` would allow a burst of 100 and
    // defeat the test, so the burst is set explicitly.
    let client = ResilientClient::builder()
        .rate_limit_with_burst(100.0, 1)
        .max_parallel(8)
        .build();
    let url = server.uri();
    let started = std::time::Instant::now();
    let calls = (0..6).map(|_| client.execute(|h| h.get(&url)));
    futures_util::future::join_all(calls).await;
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(45),
        "6 requests at 100/s with burst 1 need ≥ 50 ms, took {elapsed:?}"
    );
    assert_eq!(requests(&server).await, 6);
}

#[tokio::test]
async fn observer_ms_excludes_the_rate_wait() {
    let server = MockServer::start().await;
    mount_status(&server, 200).await;

    let rec = Arc::new(Recorder::default());
    let client = ResilientClient::builder()
        .observer("local", rec.clone())
        .rate_limit_with_burst(10.0, 1)
        .build();
    let url = server.uri();

    // First call spends the initial token immediately.
    client
        .execute(|h| h.get(&url))
        .await
        .expect("first call has the initial token");
    // Second call must wait ~100 ms for the bucket to refill.
    let started = std::time::Instant::now();
    client
        .execute(|h| h.get(&url))
        .await
        .expect("second call waits for a token");
    let elapsed = started.elapsed();

    assert_eq!(
        rec.tags(),
        ["started", "ok:200", "started", "ok:200"],
        "no event for the token wait"
    );
    for ms in rec.succeeded_ms() {
        assert!(ms < 60, "ms must exclude the rate wait, was {ms}");
    }
    assert!(
        elapsed >= Duration::from_millis(90),
        "second call must wait for a token, took {elapsed:?}"
    );
}
