//! Contract tests for `ResilientClient` against a wiremock server — retry,
//! backoff, auth refresh, circuit breaker, and the parallelism cap.

mod support;

use std::time::Duration;

use support::{fast_retry, local_client, mount, mount_delayed, mount_status, requests};
use vantage_api_pool::resilient::{ClientError, ErrorKind};
use wiremock::{MockServer, ResponseTemplate};

#[tokio::test]
async fn exhausted_retries_report_the_last_status_and_the_attempt_count() {
    let server = MockServer::start().await;
    mount_status(&server, 503).await;
    let client = local_client().retry(fast_retry(2)).build();
    let url = server.uri();
    let err = client.execute(|h| h.get(&url)).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Status(503));
    assert_eq!(err.attempts, 3, "1 try + 2 retries");
    assert_eq!(err.to_string(), "HTTP 503 after 3 attempts");
    assert_eq!(requests(&server).await, 3);
}

#[tokio::test]
async fn a_long_error_body_is_cut_to_2_kib_on_a_char_boundary() {
    let server = MockServer::start().await;
    // 'é' is two bytes, so the 2048-byte cap falls between whole characters.
    mount(
        &server,
        ResponseTemplate::new(400).set_body_string("é".repeat(4000)),
    )
    .await;
    let client = local_client().retry(fast_retry(0)).build();
    let url = server.uri();
    let body = client
        .execute(|h| h.get(&url))
        .await
        .unwrap_err()
        .body
        .expect("a body was sent");
    assert!(body.len() <= 2048, "capped at 2 KiB, was {}", body.len());
    assert!(body.len() > 2040, "was {}", body.len());
    assert!(body.chars().all(|c| c == 'é'));
}

#[tokio::test]
async fn circuit_breaker_opens_and_fails_fast() {
    let server = MockServer::start().await;
    mount_status(&server, 500).await;

    let client = local_client()
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

    let client = local_client().max_parallel(3).build();
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
    let client = local_client().retry(fast_retry(3)).build();
    let url = server.uri();
    let err: ClientError = client.execute(|h| h.get(&url)).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Status(404));
    assert_eq!(err.status(), Some(404));
    assert_eq!(err.attempts, 1, "4xx is not retried");
    assert_eq!(err.to_string(), "HTTP 404 after 1 attempt");
    assert_eq!(err.body, None, "an empty response body is no body");
}
