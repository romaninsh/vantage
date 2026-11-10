//! Insert delivery: retries only when a repeat is safe, byte-identical
//! bodies across attempts, at most five attempts, `Retry-After` honoured.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ciborium::Value as CborValue;
use tokio::time::Instant;
use uuid::Uuid;
use vantage_api_client::{ApiInsertBlock, ResponseShape, RestApi, RestInsertEndpoint};
use vantage_types::Record;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// A paused clock jumps to the next timer whenever the runtime waits on a
/// socket, so the client keeps no pooled connections and no keepalive timers
/// that could fire while a request is in flight.
fn no_idle_timers_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(0)
        .tcp_keepalive(None)
        .build()
        .unwrap()
}

fn endpoint(server: &MockServer, block: &str) -> RestInsertEndpoint {
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .http_client(no_idle_timers_client())
        .no_request_timeout()
        .build();
    let block: ApiInsertBlock = serde_yaml_ng::from_str(block).unwrap();
    api.insert_endpoint(&block, "events").unwrap()
}

fn record(name: &str) -> Record<CborValue> {
    Record::from_iter([("name".to_string(), CborValue::Text(name.to_string()))])
}

/// Answers `failures` times with `status`, then 204.
async fn failing_then_ok(server: &MockServer, status: u16, failures: usize) {
    let seen = AtomicUsize::new(0);
    Mock::given(method("POST"))
        .respond_with(move |_: &Request| {
            if seen.fetch_add(1, Ordering::SeqCst) < failures {
                ResponseTemplate::new(status)
            } else {
                ResponseTemplate::new(204)
            }
        })
        .mount(server)
        .await;
}

async fn received(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.unwrap()
}

fn header_of(request: &Request, name: &str) -> Option<String> {
    request
        .headers
        .get(name)
        .map(|v| v.to_str().unwrap().to_string())
}

#[tokio::test(start_paused = true)]
async fn an_insert_without_idempotency_is_not_retried() {
    let server = MockServer::start().await;
    failing_then_ok(&server, 503, 100).await;
    let ep = endpoint(&server, "{}");

    assert!(ep.send(&[record("a")]).await.is_err());
    assert_eq!(received(&server).await.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn an_insert_with_an_idempotency_key_retries_with_the_same_key() {
    let server = MockServer::start().await;
    failing_then_ok(&server, 503, 2).await;
    let ep = endpoint(&server, "{ batch: 2, idempotency_key: Idempotency-Key }");
    let ids = [Uuid::now_v7(), Uuid::now_v7()];

    ep.send_with_ids(&[record("a"), record("b")], &ids)
        .await
        .unwrap();

    let requests = received(&server).await;
    assert_eq!(requests.len(), 3);
    for r in &requests {
        assert_eq!(
            header_of(r, "idempotency-key"),
            Some(ids[0].to_string()),
            "every attempt carries the first record's id"
        );
        assert_eq!(r.body, requests[0].body);
    }
}

#[tokio::test(start_paused = true)]
async fn an_idempotent_insert_retries_with_identical_bytes() {
    let server = MockServer::start().await;
    failing_then_ok(&server, 500, 2).await;
    let ep = endpoint(&server, "{ idempotent: true, batch: 2, encoding: gzip }");

    ep.send(&[record("a"), record("b")]).await.unwrap();

    let requests = received(&server).await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].body, requests[0].body);
    assert_eq!(requests[2].body, requests[0].body);
}

#[tokio::test(start_paused = true)]
async fn retries_stop_after_five_attempts() {
    let server = MockServer::start().await;
    failing_then_ok(&server, 503, 100).await;
    let ep = endpoint(&server, "{ idempotent: true }");

    assert!(ep.send(&[record("a")]).await.is_err());
    assert_eq!(received(&server).await.len(), 5);
}

#[tokio::test(start_paused = true)]
async fn a_client_error_is_not_retried_even_when_idempotent() {
    let server = MockServer::start().await;
    failing_then_ok(&server, 400, 100).await;
    let ep = endpoint(&server, "{ idempotent: true }");

    assert!(ep.send(&[record("a")]).await.is_err());
    assert_eq!(received(&server).await.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn retry_after_is_honoured_up_to_thirty_seconds() {
    for (asked, waited) in [(10, 10), (120, 30)] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", asked.to_string()))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let ep = endpoint(&server, "{ idempotent: true }");

        let started = Instant::now();
        ep.send(&[record("a")]).await.unwrap();

        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_secs(waited) && elapsed < Duration::from_secs(waited + 1),
            "Retry-After {asked} waited {elapsed:?}"
        );
    }
}
