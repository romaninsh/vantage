//! The per-client token bucket: it spaces attempts out, and the waiting for a
//! token is never charged to the request.

mod support;

use std::sync::Arc;
use std::time::Duration;

use support::{Recorder, local_client, mount_status};
use wiremock::MockServer;

#[tokio::test]
async fn observer_ms_excludes_the_rate_wait() {
    let server = MockServer::start().await;
    mount_status(&server, 200).await;

    let rec = Arc::new(Recorder::default());
    let client = local_client()
        .observer("local", rec.clone())
        .rate_limit_with_burst(10.0, 1)
        .build();
    let url = server.uri();

    // First call spends the initial token immediately. The HTTP client is
    // supplied so no lazy build runs while the bucket refills.
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
