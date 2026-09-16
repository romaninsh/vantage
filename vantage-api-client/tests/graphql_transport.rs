//! `GraphqlApi` on the resilient transport.

mod support;

use std::sync::Arc;
use std::time::Duration;

use serde_json::Map;
use support::{
    Recorder, client_with_user_agent, mount, mount_n, mount_with_header, recorder, requests,
};
use vantage_api_client::{GraphqlApi, Priority};
use wiremock::MockServer;

fn api(server: &MockServer, rec: &Arc<Recorder>) -> GraphqlApi {
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
    assert!(rec.keys().iter().all(|k| k == "gql"));
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
    mount_with_header(
        &server,
        "POST",
        ("user-agent", "probe-agent"),
        200,
        r#"{"data":{"missions":[]}}"#,
    )
    .await;
    let api = GraphqlApi::builder(server.uri())
        .client(client_with_user_agent("probe-agent"))
        .build();
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
