//! Contract tests for `ResilientClient` against a wiremock server — retry,
//! backoff, auth refresh, circuit breaker, and the parallelism cap.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use vantage_api_pool::resilient::AuthRefresher;
use vantage_api_pool::{ResilientClient, RetryPolicy};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fast_retry(max: usize) -> RetryPolicy {
    RetryPolicy {
        max_retries: max,
        base_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(5),
    }
}

#[tokio::test]
async fn retries_429_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;

    let client = ResilientClient::builder().retry(fast_retry(5)).build();
    let url = format!("{}/x", server.uri());
    let resp = client.execute(|h| h.get(&url)).await.expect("succeeds after retries");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(server.received_requests().await.unwrap().len(), 3, "2×429 + 1×200");
}

#[tokio::test]
async fn retries_500_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = ResilientClient::builder().retry(fast_retry(5)).build();
    let url = server.uri();
    let resp = client.execute(|h| h.get(&url)).await.expect("succeeds after 503");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn does_not_retry_4xx() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = ResilientClient::builder().retry(fast_retry(5)).build();
    let url = server.uri();
    let r = client.execute(|h| h.get(&url)).await;
    assert!(r.is_err(), "404 is terminal");
    assert_eq!(server.received_requests().await.unwrap().len(), 1, "no retry on 404");
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
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let calls = Arc::new(AtomicUsize::new(0));
    let calls2 = calls.clone();
    let refresher: AuthRefresher = Arc::new(move || {
        let calls = calls2.clone();
        Box::pin(async move {
            // First acquire hands out a stale token; the re-acquire hands out the good one.
            let n = calls.fetch_add(1, Ordering::SeqCst);
            Ok(if n == 0 { "stale".to_string() } else { "good".to_string() })
        })
    });

    let client = ResilientClient::builder()
        .retry(fast_retry(2))
        .bearer_auth(refresher)
        .build();
    let url = server.uri();
    let resp = client.execute(|h| h.get(&url)).await.expect("succeeds after token refresh");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(calls.load(Ordering::SeqCst), 2, "acquired once, refreshed once");
}

#[tokio::test]
async fn circuit_breaker_opens_and_fails_fast() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

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
        server.received_requests().await.unwrap().len(),
        2,
        "breaker short-circuited the third request"
    );
}

#[tokio::test]
async fn caps_parallelism() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(100)))
        .mount(&server)
        .await;

    let client = ResilientClient::builder().max_parallel(3).build();
    let url = server.uri();

    let mut handles = Vec::new();
    for _ in 0..10 {
        let c = client.clone();
        let url = url.clone();
        handles.push(tokio::spawn(async move { c.execute(move |h| h.get(&url)).await }));
    }
    for h in handles {
        let _ = h.await.unwrap();
    }

    let peak = client.peak_in_flight();
    assert!(peak <= 3, "parallelism cap exceeded: peak {peak}");
    assert!(peak >= 2, "requests did not actually parallelize: peak {peak}");
}
