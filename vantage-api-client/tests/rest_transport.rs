//! `RestApi` on the resilient transport: the retry policy follows the
//! `Priority` task-local, errors carry the status, rows pulled are reported.

mod support;

use std::time::Duration;

use support::{mount, mount_n, recorder, requests};
use vantage_api_client::{Priority, ResponseShape, RestApi};
use vantage_dataset::prelude::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use wiremock::MockServer;

fn api(server: &MockServer, rec: &std::sync::Arc<support::Recorder>) -> RestApi {
    RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .observer("local", rec.clone())
        .build()
}

/// Rows through the public surface: a `Table` over the API, listed.
async fn list_rows(api: &RestApi) -> vantage_core::Result<usize> {
    let table: Table<RestApi, EmptyEntity> = Table::new("things", api.clone()).with_id_column("id");
    Ok(table.list_values().await?.len())
}

#[tokio::test]
async fn background_makes_one_attempt_and_reports_the_status() {
    let server = MockServer::start().await;
    mount(&server, "GET", 503, "").await;
    let rec = recorder();
    let api = api(&server, &rec);
    let err = list_rows(&api).await.unwrap_err();
    let text = err.to_string();
    assert!(text.contains("503"), "status in the error: {text}");
    assert_eq!(requests(&server).await, 1);
    assert_eq!(rec.tags(), ["started", "failed:status"]);
}

#[tokio::test]
async fn essential_retries_until_the_server_answers() {
    let server = MockServer::start().await;
    mount_n(&server, "GET", 503, "", 2).await;
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
    assert!(tags.contains(&"retry:1".to_string()));
    assert_eq!(tags.last().map(String::as_str), Some("rows:3"));
}

#[tokio::test]
async fn four_xx_is_final_and_carries_the_body() {
    let server = MockServer::start().await;
    mount(&server, "GET", 422, r#"{"detail":"bad filter"}"#).await;
    let rec = recorder();
    let api = api(&server, &rec);
    let err = Priority::Essential
        .scope(list_rows(&api))
        .await
        .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("422"), "{text}");
    assert!(
        text.contains("bad filter"),
        "body text in the error: {text}"
    );
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
    assert!(rec.keys.lock().unwrap().iter().all(|k| k == "local"));
    assert_eq!(
        api.breaker_state(),
        Some(vantage_api_client::BreakerState::Closed)
    );
}

#[tokio::test]
async fn http_request_sends_headers_and_body_and_reports_a_write() {
    let server = MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::header("Idempotency-Key", "abc-123"))
        .and(wiremock::matchers::body_json(
            serde_json::json!({"name": "widget"}),
        ))
        .respond_with(wiremock::ResponseTemplate::new(201))
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
async fn breaker_opens_after_five_background_failures() {
    let server = MockServer::start().await;
    mount(&server, "GET", 503, "").await;
    let rec = recorder();
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .max_parallel(1)
        .observer("local", rec.clone())
        .build();

    // `default_breaker` opens after 5 consecutive failures.
    for _ in 0..5 {
        let err = list_rows(&api).await.unwrap_err();
        assert!(err.to_string().contains("503"));
    }

    let err = list_rows(&api).await.unwrap_err();
    let text = err.to_string();
    assert!(text.contains("breaker_open"), "{text}");
    assert_eq!(
        rec.tags().last().map(String::as_str),
        Some("failed:breaker_open")
    );
}

#[tokio::test]
async fn http_client_override_travels_with_every_request() {
    let server = MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::header("user-agent", "probe-agent"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    let custom = reqwest::Client::builder()
        .user_agent("probe-agent")
        .build()
        .unwrap();
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .http_client(custom)
        .build();
    assert_eq!(list_rows(&api).await.unwrap(), 0);
}

#[tokio::test]
async fn caller_authorization_header_replaces_the_configured_one() {
    let server = MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::header(
            "Authorization",
            "Bearer override",
        ))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .auth("Bearer configured")
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
async fn auth_header_still_travels() {
    let server = MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::header("Authorization", "Bearer t0k"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .auth("Bearer t0k")
        .build();
    assert_eq!(list_rows(&api).await.unwrap(), 0);
}
