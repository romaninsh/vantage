//! The request timeout the crate applies to the default client.

mod support;

use std::time::Duration;

use support::*;
use vantage_api_pool::resilient::{CallPolicy, ErrorKind};
use vantage_api_pool::{ResilientClient, RetryPolicy};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn slow_server(delay: Duration) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(delay))
        .mount(&server)
        .await;
    server
}

/// The one test that goes through the default, lazily-built client, so the
/// builder's `request_timeout` reaching it stays covered. Its first request
/// includes the system proxy lookup, which can block for seconds on macOS,
/// hence the longer deadline.
#[tokio::test]
async fn the_default_client_applies_the_request_timeout() {
    let server = slow_server(Duration::from_millis(600)).await;
    let client = ResilientClient::builder()
        .request_timeout(Duration::from_millis(100))
        .build();
    let url = server.uri();

    let err = within_secs(
        30,
        client.execute_with(&CallPolicy::background(), |h| h.get(&url)),
    )
    .await
    .expect_err("the server answers after the timeout");

    assert!(matches!(err.kind, ErrorKind::Transport(_)), "{err}");
    assert_eq!(err.attempts, 1);
}

#[tokio::test]
async fn a_response_slower_than_the_timeout_is_a_transport_error() {
    let server = slow_server(Duration::from_millis(600)).await;
    let client = local_client_with_timeout(Some(Duration::from_millis(100))).build();
    let url = server.uri();

    let err = within(client.execute_with(&CallPolicy::background(), |h| h.get(&url)))
        .await
        .expect_err("the server answers after the timeout");

    assert!(matches!(err.kind, ErrorKind::Transport(_)), "{err}");
    assert_eq!(err.attempts, 1);
}

#[tokio::test]
async fn a_timed_out_attempt_is_retried() {
    let server = slow_server(Duration::from_millis(600)).await;
    let client = local_client_with_timeout(Some(Duration::from_millis(100))).build();
    let url = server.uri();
    let policy = CallPolicy::bounded(RetryPolicy::new(
        1,
        Duration::from_millis(1),
        Duration::from_millis(5),
    ));

    let err = within(client.execute_with(&policy, |h| h.get(&url)))
        .await
        .expect_err("both attempts time out");

    assert_eq!(err.attempts, 2);
}

#[tokio::test]
async fn a_supplied_client_keeps_its_own_timeout_setting() {
    let server = slow_server(Duration::from_millis(300)).await;
    let client = ResilientClient::builder()
        .request_timeout(Duration::from_millis(50))
        .http_client(reqwest::Client::builder().no_proxy().build().unwrap())
        .build();
    let url = server.uri();

    let resp = within(client.execute_with(&CallPolicy::background(), |h| h.get(&url)))
        .await
        .expect("the supplied client has no timeout");

    assert_eq!(resp.status().as_u16(), 200);
}
