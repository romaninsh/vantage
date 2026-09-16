//! Contract tests for `ResilientClient` against a wiremock server — retry,
//! backoff, auth refresh, circuit breaker, and the parallelism cap.

mod support;

use std::time::Duration;

use support::{
    fast_retry, mount, mount_delayed, mount_script, mount_status, requests, scripted_refresher,
};
use vantage_api_pool::resilient::{ClientError, ErrorKind};
use vantage_api_pool::ResilientClient;
use wiremock::matchers::{header, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn retries_429_then_succeeds() {
    let server = MockServer::start().await;
    mount_script(&server, [429, 429, 200]).await;

    let client = ResilientClient::builder().retry(fast_retry(5)).build();
    let url = format!("{}/x", server.uri());
    let resp = client
        .execute(|h| h.get(&url))
        .await
        .expect("succeeds after retries");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(requests(&server).await, 3, "2×429 + 1×200");
}

#[tokio::test]
async fn retries_500_then_succeeds() {
    let server = MockServer::start().await;
    mount_script(&server, [503, 200]).await;

    let client = ResilientClient::builder().retry(fast_retry(5)).build();
    let url = server.uri();
    let resp = client
        .execute(|h| h.get(&url))
        .await
        .expect("succeeds after 503");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(requests(&server).await, 2);
}

#[tokio::test]
async fn does_not_retry_4xx() {
    let server = MockServer::start().await;
    mount_status(&server, 404).await;

    let client = ResilientClient::builder().retry(fast_retry(5)).build();
    let url = server.uri();
    let r = client.execute(|h| h.get(&url)).await;
    assert!(r.is_err(), "404 is terminal");
    assert_eq!(requests(&server).await, 1, "no retry on 404");
}

#[tokio::test]
async fn refreshes_token_on_401_and_replays() {
    let server = MockServer::start().await;
    // Only a request bearing the *refreshed* token succeeds.
    Mock::given(method("GET"))
        .and(header("Authorization", "Bearer good"))
        .respond_with(ResponseTemplate::new(200))
        .with_priority(1)
        .mount(&server)
        .await;
    mount_status(&server, 401).await;

    // The first acquire hands out a stale token; the re-acquire the good one.
    let (refresher, calls) =
        scripted_refresher(|n| Ok(if n == 0 { "stale" } else { "good" }.to_string()));

    let client = ResilientClient::builder()
        .retry(fast_retry(2))
        .bearer_auth(refresher)
        .build();
    let url = server.uri();
    let resp = client
        .execute(|h| h.get(&url))
        .await
        .expect("succeeds after token refresh");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(calls.count(), 2, "acquired once, refreshed once");
}

#[tokio::test]
async fn circuit_breaker_opens_and_fails_fast() {
    let server = MockServer::start().await;
    mount_status(&server, 500).await;

    let client = ResilientClient::builder()
        .retry(fast_retry(0)) // 1 request per execute
        .circuit_breaker(2, Duration::from_secs(60))
        .build();
    let url = server.uri();

    assert!(client.execute(|h| h.get(&url)).await.is_err()); // failure 1
    assert!(client.execute(|h| h.get(&url)).await.is_err()); // failure 2 → opens
    let third = client.execute(|h| h.get(&url)).await;
    assert!(third.is_err(), "breaker open → fast fail");

    // The third call never reached the server.
    assert_eq!(
        requests(&server).await,
        2,
        "breaker short-circuited the third request"
    );
}

#[tokio::test]
async fn caps_parallelism() {
    let server = MockServer::start().await;
    mount_delayed(&server, 200, Duration::from_millis(100)).await;

    let client = ResilientClient::builder().max_parallel(3).build();
    let url = server.uri();

    let mut handles = Vec::new();
    for _ in 0..10 {
        let c = client.clone();
        let url = url.clone();
        handles.push(tokio::spawn(async move {
            c.execute(move |h| h.get(&url)).await
        }));
    }
    for h in handles {
        let _ = h.await.unwrap();
    }

    let peak = client.peak_in_flight();
    assert!(peak <= 3, "parallelism cap exceeded: peak {peak}");
    assert!(
        peak >= 2,
        "requests did not actually parallelize: peak {peak}"
    );
}

#[tokio::test]
async fn error_carries_status_and_attempts() {
    let server = MockServer::start().await;
    mount_status(&server, 404).await;
    let client = ResilientClient::builder().retry(fast_retry(3)).build();
    let url = server.uri();
    let err: ClientError = client.execute(|h| h.get(&url)).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Status(404));
    assert_eq!(err.status(), Some(404));
    assert_eq!(err.attempts, 1, "4xx is not retried");
    assert_eq!(err.to_string(), "HTTP 404 after 1 attempt");
    assert_eq!(err.body, None, "an empty response body is no body");
}

#[tokio::test]
async fn error_carries_the_servers_own_message() {
    let server = MockServer::start().await;
    mount(
        &server,
        ResponseTemplate::new(422)
            .insert_header("content-type", "application/json")
            .set_body_string(r#"{"error":"name must not be empty"}"#),
    )
    .await;
    let client = ResilientClient::builder().retry(fast_retry(3)).build();
    let url = server.uri();
    let err = client.execute(|h| h.get(&url)).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Status(422));
    assert_eq!(
        err.body.as_deref(),
        Some(r#"{"error":"name must not be empty"}"#),
        "the outbox needs the server's message, not just the status"
    );
    assert_eq!(
        err.to_string(),
        "HTTP 422 after 1 attempt",
        "the body is not part of Display"
    );
}

#[tokio::test]
async fn a_long_error_body_is_capped() {
    let server = MockServer::start().await;
    mount(
        &server,
        ResponseTemplate::new(400).set_body_string("é".repeat(4000)),
    )
    .await;
    let client = ResilientClient::builder().retry(fast_retry(0)).build();
    let url = server.uri();
    let body = client
        .execute(|h| h.get(&url))
        .await
        .unwrap_err()
        .body
        .expect("a body was sent");
    assert!(body.len() <= 2048, "capped at 2 KiB, was {}", body.len());
    assert!(
        body.len() > 2040,
        "cut back only as far as the nearest char boundary, was {}",
        body.len()
    );
    assert!(body.chars().all(|c| c == 'é'), "cut on a char boundary");
}

#[tokio::test]
async fn exhausted_retries_report_last_status_and_attempt_count() {
    let server = MockServer::start().await;
    mount_status(&server, 503).await;
    let client = ResilientClient::builder().retry(fast_retry(2)).build();
    let url = server.uri();
    let err = client.execute(|h| h.get(&url)).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Status(503));
    assert_eq!(err.attempts, 3, "1 try + 2 retries");
    assert_eq!(err.to_string(), "HTTP 503 after 3 attempts");
}
