//! Per-call policies: background calls never retry, essential calls retry
//! until the caller drops them, and 4xx is final under every mode.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use vantage_api_pool::resilient::{BreakerMode, CallPolicy, ErrorKind, RetryMode};
use vantage_api_pool::{ResilientClient, RetryPolicy};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn essential_fast() -> CallPolicy {
    CallPolicy {
        retry: RetryMode::UntilCancelled {
            base: Duration::from_millis(1),
            max: Duration::from_millis(4),
        },
        breaker: BreakerMode::WaitForProbe,
    }
}

#[tokio::test]
async fn background_call_makes_exactly_one_attempt() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let client = ResilientClient::builder().build();
    let url = server.uri();
    let err = client
        .execute_with(&CallPolicy::background(), |h| h.get(&url))
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Status(503));
    assert_eq!(err.attempts, 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn essential_call_retries_until_the_server_recovers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(7)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let client = ResilientClient::builder().build();
    let url = server.uri();
    let resp = client
        .execute_with(&essential_fast(), |h| h.get(&url))
        .await
        .expect("recovers after 7 failures");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(server.received_requests().await.unwrap().len(), 8);
}

#[tokio::test]
async fn essential_call_stops_when_dropped() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let client = ResilientClient::builder().build();
    let url = server.uri();
    let task = tokio::spawn({
        let client = client.clone();
        async move {
            let _ = client.execute_with(&essential_fast(), |h| h.get(&url)).await;
        }
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    task.abort();
    let _ = task.await;
    let seen = server.received_requests().await.unwrap().len();
    assert!(seen >= 3, "was retrying while alive: {seen}");
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        seen,
        "no request after the caller dropped the future"
    );
}

#[tokio::test]
async fn four_xx_is_final_even_for_essential_calls() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let client = ResilientClient::builder().build();
    let url = server.uri();
    let err = client
        .execute_with(&essential_fast(), |h| h.get(&url))
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Status(404));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn bounded_policy_per_call_overrides_client_default() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let client = ResilientClient::builder()
        .retry(RetryPolicy {
            max_retries: 9,
            base_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(1),
        })
        .build();
    let url = server.uri();
    let policy = CallPolicy::bounded(RetryPolicy {
        max_retries: 1,
        base_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(1),
    });
    let err = client
        .execute_with(&policy, |h| h.get(&url))
        .await
        .unwrap_err();
    assert_eq!(err.attempts, 2);
}

#[tokio::test]
async fn execute_keeps_the_client_default_policy() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let client = ResilientClient::builder()
        .retry(RetryPolicy {
            max_retries: 2,
            base_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(1),
        })
        .build();
    let url = server.uri();
    let err = client.execute(|h| h.get(&url)).await.unwrap_err();
    assert_eq!(err.attempts, 3);
}

/// Shared helper for later tasks: counts requests the server saw.
#[allow(dead_code)]
fn hits(counter: &Arc<AtomicUsize>) -> usize {
    counter.load(Ordering::SeqCst)
}
