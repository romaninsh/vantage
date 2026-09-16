//! `GraphqlApi` on the resilient transport.

mod support;

use std::time::Duration;

use serde_json::Map;
use support::{mount, mount_n, recorder, requests};
use vantage_api_client::{GraphqlApi, Priority};
use wiremock::MockServer;

fn api(server: &MockServer, rec: &std::sync::Arc<support::Recorder>) -> GraphqlApi {
    GraphqlApi::builder(server.uri())
        .observer("gql", rec.clone())
        .build()
}

#[tokio::test]
async fn background_post_makes_one_attempt_and_keeps_the_body() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        400,
        r#"{"errors":[{"message":"Unknown argument find"}]}"#,
    )
    .await;
    let rec = recorder();
    let api = api(&server, &rec);
    let err = api
        .post_graphql("{ missions { id } }", &Map::new())
        .await
        .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("400"), "{text}");
    assert!(
        text.contains("Unknown argument"),
        "server message kept: {text}"
    );
    assert_eq!(requests(&server).await, 1);
    assert_eq!(rec.tags(), ["started", "failed:status"]);
    assert!(rec.keys.lock().unwrap().iter().all(|k| k == "gql"));
}

#[tokio::test]
async fn essential_post_retries_5xx() {
    let server = MockServer::start().await;
    mount_n(&server, "POST", 502, "", 2).await;
    mount(
        &server,
        "POST",
        200,
        r#"{"data":{"missions":[{"id":"m1"}]}}"#,
    )
    .await;
    let rec = recorder();
    let api = api(&server, &rec);
    let data = tokio::time::timeout(
        Duration::from_secs(5),
        Priority::Essential.scope(api.post_graphql("{ missions { id } }", &Map::new())),
    )
    .await
    .expect("finishes")
    .expect("succeeds");
    assert_eq!(data["missions"][0]["id"], "m1");
    assert_eq!(requests(&server).await, 3);
}

#[tokio::test]
async fn http_client_override_travels_with_every_request() {
    let server = MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::header("user-agent", "probe-agent"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_string(r#"{"data":{"missions":[]}}"#),
        )
        .mount(&server)
        .await;
    let custom = reqwest::Client::builder()
        .user_agent("probe-agent")
        .build()
        .unwrap();
    let api = GraphqlApi::builder(server.uri()).client(custom).build();
    let data = api
        .post_graphql("{ missions { id } }", &Map::new())
        .await
        .unwrap();
    assert_eq!(data["missions"], serde_json::json!([]));
}

#[tokio::test]
async fn graphql_errors_in_a_200_are_not_retried() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        200,
        r#"{"data":null,"errors":[{"message":"nope"}]}"#,
    )
    .await;
    let rec = recorder();
    let api = api(&server, &rec);
    let err = Priority::Essential
        .scope(api.post_graphql("{ x }", &Map::new()))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("nope"));
    assert_eq!(requests(&server).await, 1);
    assert_eq!(rec.tags(), ["started", "ok:200"]);
}
