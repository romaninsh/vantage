//! `RestApi` on the resilient transport: the retry policy follows the
//! `Priority` task-local, errors carry the status, rows pulled are reported.

mod support;

use std::sync::Arc;
use std::time::Duration;

use support::{
    Recorder, client_with_user_agent, mount, mount_n, mount_with_header, recorder, requests,
    test_client,
};
use vantage_api_client::{Priority, ResponseShape, RestApi};
use vantage_dataset::prelude::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use wiremock::matchers::{body_json, header, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn api(server: &MockServer, rec: &Arc<Recorder>) -> RestApi {
    RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .observer("local", rec.clone())
        .http_client(test_client())
        .build()
}

/// Rows through the public surface: a `Table` over the API, listed.
async fn list_rows(api: &RestApi) -> vantage_core::Result<usize> {
    let table: Table<RestApi, EmptyEntity> = Table::new("things", api.clone()).with_id_column("id");
    Ok(table.list_values().await?.len())
}

#[tokio::test]
async fn essential_retries_5xx_and_429_until_the_server_answers() {
    let server = MockServer::start().await;
    mount_n(&server, "GET", 503, "", 1).await;
    mount_n(&server, "GET", 429, "", 1).await;
    mount(&server, "GET", 200, r#"[{"id":"a"},{"id":"b"},{"id":"c"}]"#).await;
    let rec = recorder();
    let api = api(&server, &rec);
    let n = tokio::time::timeout(
        Duration::from_secs(5),
        Priority::Essential.scope(list_rows(&api)),
    )
    .await
    .expect("finishes")
    .expect("succeeds after retries");
    assert_eq!(n, 3);
    assert_eq!(requests(&server).await, 3);
    let tags = rec.tags();
    assert_eq!(tags.first().map(String::as_str), Some("started"));
    assert!(tags.contains(&"retry:1".to_string()), "{tags:?}");
    assert!(tags.contains(&"retry:2".to_string()), "{tags:?}");
    assert_eq!(tags.last().map(String::as_str), Some("rows:3"));
}

#[tokio::test]
async fn four_xx_is_final_and_carries_the_body_cut_to_500_characters() {
    let server = MockServer::start().await;
    let body = format!("bad filter{}{}", "a".repeat(490), "z".repeat(500));
    mount(&server, "GET", 422, &body).await;
    let rec = recorder();
    let api = api(&server, &rec);
    let text = Priority::Essential
        .scope(list_rows(&api))
        .await
        .unwrap_err()
        .to_string();
    assert!(text.contains("422"), "{text}");
    assert!(
        text.contains(&body[..500]),
        "the first 500 characters are kept: {text}"
    );
    assert!(!text.contains('z'), "the rest is dropped: {text}");
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test]
async fn observer_key_is_the_configured_one_and_rows_are_counted() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, r#"[{"id":"1"},{"id":"2"}]"#).await;
    let rec = recorder();
    let api = api(&server, &rec);
    assert_eq!(list_rows(&api).await.unwrap(), 2);
    assert_eq!(rec.tags(), ["started", "ok:200", "rows:2"]);
    assert!(rec.keys().iter().all(|k| k == "local"));
    assert_eq!(
        api.breaker_state(),
        Some(vantage_api_client::BreakerState::Closed)
    );
}

#[tokio::test]
async fn http_request_sends_headers_and_body_and_reports_a_write() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("Idempotency-Key", "abc-123"))
        .and(body_json(serde_json::json!({"name": "widget"})))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;
    let rec = recorder();
    let api = api(&server, &rec);
    let body = serde_json::json!({"name": "widget"});
    let response = api
        .http_request(
            reqwest::Method::POST,
            "things",
            &[("Idempotency-Key", "abc-123")],
            Some(&body),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(rec.tags(), ["started", "ok:201", "write"]);
}

#[tokio::test]
async fn background_makes_one_attempt_and_five_failures_open_the_breaker() {
    let server = MockServer::start().await;
    mount(&server, "GET", 503, "").await;
    let rec = recorder();
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .max_parallel(1)
        .observer("local", rec.clone())
        .http_client(test_client())
        .build();

    // `default_breaker` opens after 5 consecutive failures.
    for _ in 0..5 {
        let err = list_rows(&api).await.unwrap_err();
        assert!(
            err.to_string().contains("503"),
            "status in the error: {err}"
        );
    }
    let err = list_rows(&api).await.unwrap_err();
    assert!(err.to_string().contains("breaker_open"), "{err}");

    assert_eq!(
        requests(&server).await,
        5,
        "one attempt per call, none once open"
    );
    let tags = rec.tags();
    assert_eq!(tags[..2], ["started", "failed:status"]);
    assert_eq!(tags.last().map(String::as_str), Some("failed:breaker_open"));
}

#[tokio::test]
async fn caller_authorization_header_replaces_the_configured_one() {
    let server = MockServer::start().await;
    mount_with_header(
        &server,
        "POST",
        ("Authorization", "Bearer override"),
        200,
        "",
    )
    .await;
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .auth("Bearer configured")
        .http_client(test_client())
        .build();

    api.http_request(
        reqwest::Method::POST,
        "things",
        &[("Authorization", "Bearer override")],
        None,
    )
    .await
    .unwrap();

    let received = server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1, "exactly one request matched");
    let auth_values: Vec<_> = received[0]
        .headers
        .get_all("authorization")
        .iter()
        .collect();
    assert_eq!(
        auth_values.len(),
        1,
        "the request must carry a single Authorization header, not two"
    );
    assert_eq!(auth_values[0], "Bearer override");
}

#[tokio::test]
async fn auth_refresher_supplies_the_token_and_is_asked_again_on_401() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let server = MockServer::start().await;
    mount_with_header(&server, "GET", ("Authorization", "Bearer stale"), 401, "").await;
    mount_with_header(
        &server,
        "GET",
        ("Authorization", "Bearer fresh"),
        200,
        r#"[{"id":"a"}]"#,
    )
    .await;
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    let refresher: vantage_api_client::AuthRefresher = Arc::new(move || {
        let n = counter.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(if n == 0 { "stale" } else { "fresh" }.to_string()) })
    });
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .auth_refresher(refresher)
        .http_client(test_client())
        .build();

    assert_eq!(list_rows(&api).await.unwrap(), 1);
    assert_eq!(list_rows(&api).await.unwrap(), 1);
    assert_eq!(
        asked.load(Ordering::SeqCst),
        2,
        "once to start, once on the 401"
    );
}

/// The mock answers only a request carrying the overriding client's
/// `user-agent` and the fixed `authorization`; a fixed `auth` set after a
/// refresher replaces it, so the refresher is never asked.
#[tokio::test]
async fn the_configured_client_and_fixed_auth_reach_the_request() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(header("user-agent", "probe-agent"))
        .and(header("Authorization", "Bearer fixed"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"[{"id":"a"}]"#))
        .mount(&server)
        .await;
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    let refresher: vantage_api_client::AuthRefresher = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok("refreshed".to_string()) })
    });
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .auth_refresher(refresher)
        .auth("Bearer fixed")
        .http_client(client_with_user_agent("probe-agent"))
        .build();

    assert_eq!(list_rows(&api).await.unwrap(), 1);
    assert_eq!(asked.load(Ordering::SeqCst), 0, "the refresher is gone");
}

#[tokio::test]
async fn too_many_requests_never_opens_the_breaker() {
    let server = MockServer::start().await;
    mount(&server, "GET", 429, "").await;
    let rec = recorder();
    let api = api(&server, &rec);
    for _ in 0..7 {
        let err = list_rows(&api).await.unwrap_err();
        assert!(err.to_string().contains("429"), "{err}");
    }
    assert_eq!(requests(&server).await, 7, "every call reached the server");
    assert_eq!(
        api.breaker_state(),
        Some(vantage_api_client::BreakerState::Closed)
    );
}

#[tokio::test]
async fn a_401_after_the_fresh_token_is_a_final_error() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let server = MockServer::start().await;
    mount(&server, "GET", 401, "denied").await;
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    let refresher: vantage_api_client::AuthRefresher = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok("token".to_string()) })
    });
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .auth_refresher(refresher)
        .http_client(test_client())
        .build();

    let err = list_rows(&api).await.unwrap_err().to_string();
    assert!(err.contains("401"), "{err}");
    assert_eq!(
        asked.load(Ordering::SeqCst),
        2,
        "replayed once with a fresh token"
    );
    assert_eq!(requests(&server).await, 2);
}

#[tokio::test]
async fn a_failure_with_no_response_is_a_transport_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let rec = recorder();
    let api = RestApi::builder(uri)
        .response_shape(ResponseShape::BareArray)
        .observer("local", rec.clone())
        .http_client(test_client())
        .build();
    let err = list_rows(&api).await.unwrap_err().to_string();
    assert!(err.contains("transport"), "{err}");
    assert_eq!(rec.tags(), ["started", "failed:transport"]);
}
