//! Per-call policies: background calls never retry, essential calls retry
//! until the caller drops them, 4xx is final under every mode, and a dropped
//! future leaves nothing behind.

mod support;

use std::sync::Arc;
use std::time::Duration;

use support::{
    Recorder, cancel, essential_fast, local_client, mount_delayed, mount_script, mount_status,
    requests, scripted_refresher, spawn_essential, wait_for_requests, wait_until, within,
};
use vantage_api_pool::RetryPolicy;
use vantage_api_pool::resilient::CallPolicy;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn background_call_makes_exactly_one_attempt() {
    let server = MockServer::start().await;
    mount_status(&server, 503).await;
    let client = local_client().build();
    let url = server.uri();
    let err = client
        .execute_with(&CallPolicy::background(), |h| h.get(&url))
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(503));
    assert_eq!(err.attempts, 1);
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test]
async fn the_stock_essential_policy_retries_until_the_server_recovers() {
    let server = MockServer::start().await;
    mount_script(&server, [503, 503, 200]).await;
    let client = local_client().build();
    let url = server.uri();
    let resp = within(client.execute_with(&CallPolicy::essential(), |h| h.get(&url)))
        .await
        .expect("recovers after 2 failures");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(requests(&server).await, 3);
}

#[tokio::test]
async fn essential_call_retries_until_the_server_recovers() {
    let server = MockServer::start().await;
    mount_script(&server, [503, 503, 503, 503, 503, 503, 503, 200]).await;
    let client = local_client().build();
    let url = server.uri();
    let resp = within(client.execute_with(&essential_fast(), |h| h.get(&url)))
        .await
        .expect("recovers after 7 failures");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(requests(&server).await, 8);
}

#[tokio::test]
async fn cancelled_call_leaves_nothing_in_flight() {
    let server = MockServer::start().await;
    mount_delayed(&server, 200, Duration::from_millis(300)).await;
    let client = local_client().build();
    let url = server.uri();
    let task = spawn_essential(&client, &url);
    wait_for_requests(&server, 1, Duration::from_secs(2)).await;
    assert_eq!(client.in_flight(), 1, "the request is in flight");
    cancel(task).await;
    wait_until(
        Duration::from_secs(2),
        "the in-flight count returns to 0",
        || client.in_flight() == 0,
    )
    .await;
    assert_eq!(client.peak_in_flight(), 1);
}

#[tokio::test]
async fn a_cancelled_call_reports_cancelled_as_its_terminal_event() {
    let server = MockServer::start().await;
    mount_delayed(&server, 200, Duration::from_millis(300)).await;
    let rec = Arc::new(Recorder::default());
    let client = local_client().observer("local", rec.clone()).build();
    let url = server.uri();
    let task = spawn_essential(&client, &url);
    wait_for_requests(&server, 1, Duration::from_secs(2)).await;
    cancel(task).await;
    wait_until(
        Duration::from_secs(2),
        "the observer hears the cancellation",
        || rec.tags() == ["started", "cancelled"],
    )
    .await;
}

#[tokio::test]
async fn retry_after_is_clamped_to_the_policy_ceiling() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "120"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    mount_status(&server, 200).await;
    let client = local_client().build();
    let url = server.uri();
    let policy = CallPolicy::bounded(RetryPolicy::new(
        1,
        Duration::from_millis(1),
        Duration::from_millis(20),
    ));

    let started = std::time::Instant::now();
    let resp = within(client.execute_with(&policy, |h| h.get(&url)))
        .await
        .expect("retries once and succeeds");
    let elapsed = started.elapsed();

    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(requests(&server).await, 2);
    assert!(
        elapsed < Duration::from_secs(1),
        "the server's 120 s Retry-After must be clamped to the policy's 20 ms ceiling, took {elapsed:?}"
    );
}

#[tokio::test]
async fn a_failing_refresher_fails_the_call_before_any_request() {
    let server = MockServer::start().await;
    mount_status(&server, 200).await;

    let (refresher, calls) = scripted_refresher(|_| Err(anyhow::anyhow!("token endpoint down")));
    let client = local_client().bearer_auth(refresher).build();
    let url = server.uri();
    let err = within(client.execute_with(&essential_fast(), |h| h.get(&url)))
        .await
        .unwrap_err();

    assert_eq!(err.kind_name(), "auth");
    assert_eq!(err.attempts, 1, "an auth failure is not retried");
    assert_eq!(err.body, None);
    assert_eq!(err.to_string(), "auth refresh failed: token endpoint down");
    assert_eq!(calls.count(), 1);
    assert_eq!(
        requests(&server).await,
        0,
        "no request goes out without a token"
    );
}
