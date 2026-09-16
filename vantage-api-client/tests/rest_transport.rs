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
