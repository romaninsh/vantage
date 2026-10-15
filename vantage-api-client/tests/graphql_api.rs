//! `GraphqlApi` and its builder: what it reports about itself, what the
//! `Supports` switches default to per dialect, and the transport knobs.

mod support;

use std::time::Duration;

use serde_json::{Map, json};
use support::{mount, mount_with_header, recorder, requests, test_client};
use vantage_api_client::{BreakerState, CountMode, FilterDialect, GraphqlApi, Priority, Supports};
use wiremock::MockServer;

#[test]
fn an_api_reports_its_endpoint_and_defaults_to_the_generic_dialect() {
    let api = GraphqlApi::new("https://api.spacex.land/graphql/");
    assert_eq!(api.endpoint(), "https://api.spacex.land/graphql/");
    assert_eq!(api.dialect(), FilterDialect::Generic);
    assert!(api.root_args().is_none());
    assert!(api.response_path().is_empty());
}

#[test]
fn the_builder_sets_the_dialect_root_args_and_response_path() {
    let api = GraphqlApi::builder("https://example.test/graphql")
        .dialect(FilterDialect::Hasura)
        .root_args(json!({ "input": {} }))
        .response_path("edges.node")
        .build();
    assert_eq!(api.dialect(), FilterDialect::Hasura);
    assert_eq!(api.root_args(), Some(&json!({ "input": {} })));
    assert_eq!(api.response_path(), ["edges", "node"]);
}

#[test]
fn stray_dots_in_a_response_path_name_no_segment() {
    let api = GraphqlApi::builder("https://example.test/graphql")
        .response_path(".edges..node.")
        .build();
    assert_eq!(api.response_path(), ["edges", "node"]);
}

#[test]
fn debug_output_keeps_the_auth_header_secret() {
    let api = GraphqlApi::builder("https://example.test/graphql")
        .auth("Bearer secret-token")
        .build();
    let text = format!("{api:?}");
    assert!(!text.contains("secret-token"), "{text}");
}

#[test]
fn generic_defaults_to_filtering_only() {
    let api = GraphqlApi::new("https://example.test/graphql");
    assert!(api.can_filter());
    assert!(!api.can_order());
    assert!(!api.can_search());
    assert!(!api.can_filter_operators());
    assert!(!api.can_paginate());
    assert!(!api.can_page());
}

#[test]
fn hasura_defaults_to_filter_order_search_and_operators_but_not_paging() {
    let api = GraphqlApi::builder("https://example.test/graphql")
        .dialect(FilterDialect::Hasura)
        .build();
    assert!(api.can_filter());
    assert!(api.can_order());
    assert!(api.can_search());
    assert!(api.can_filter_operators());
    assert!(!api.can_paginate());
    assert!(!api.can_page());
}

#[test]
fn supports_overrides_the_dialect_defaults() {
    let api = GraphqlApi::builder("https://example.test/graphql")
        .supports(
            Supports::default()
                .with_order(true)
                .with_search(true)
                .with_paginate(true),
        )
        .build();
    assert!(api.can_order());
    assert!(api.can_paginate());
    assert!(api.can_page());
    assert!(api.can_search(), "search was switched on for Generic");
    assert!(!api.can_filter_operators(), "operators stay Hasura-only");

    let hasura = GraphqlApi::builder("https://example.test/graphql")
        .dialect(FilterDialect::Hasura)
        .supports(Supports::default().with_order(false).with_search(false))
        .build();
    assert!(!hasura.can_order());
    assert!(!hasura.can_search());
}

#[test]
fn without_filter_push_down_nothing_condition_shaped_is_offered() {
    let api = GraphqlApi::builder("https://example.test/graphql")
        .dialect(FilterDialect::Hasura)
        .supports(Supports::default().with_filter(false).with_paginate(true))
        .build();
    assert!(!api.can_filter());
    assert!(!api.can_search());
    assert!(!api.can_filter_operators());
    assert!(api.can_paginate());
    assert!(
        !api.can_page(),
        "a page cut by local filtering would be short"
    );
}

#[test]
fn supports_setters_record_what_was_set() {
    let supports = Supports::default()
        .with_filter(false)
        .with_order(true)
        .with_search(false)
        .with_paginate(true)
        .with_count(CountMode::Aggregate);
    assert_eq!(supports.filter, Some(false));
    assert_eq!(supports.order, Some(true));
    assert_eq!(supports.search, Some(false));
    assert_eq!(supports.paginate, Some(true));
    assert_eq!(supports.count, CountMode::Aggregate);

    let unset = Supports::default();
    assert_eq!(unset.filter, None);
    assert_eq!(unset.count, CountMode::List);
}

#[test]
fn count_mode_and_dialect_are_lowercase_in_yaml() {
    let mode: CountMode = serde_yaml_ng::from_str("aggregate").unwrap();
    assert_eq!(mode, CountMode::Aggregate);
    let mode: CountMode = serde_yaml_ng::from_str("list").unwrap();
    assert_eq!(mode, CountMode::List);
    assert!(serde_yaml_ng::from_str::<CountMode>("exact").is_err());

    let dialect: FilterDialect = serde_yaml_ng::from_str("hasura").unwrap();
    assert_eq!(dialect, FilterDialect::Hasura);
    assert!(serde_yaml_ng::from_str::<FilterDialect>("Hasura").is_err());
    assert!(serde_yaml_ng::from_str::<FilterDialect>("relay").is_err());
}

#[tokio::test]
async fn the_breaker_starts_closed() {
    let api = GraphqlApi::builder("https://example.test/graphql")
        .http_client(test_client())
        .build();
    assert_eq!(api.breaker_state(), Some(BreakerState::Closed));
    let _client = api.client();
}

#[tokio::test]
async fn the_configured_transport_knobs_still_deliver_the_request() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, r#"{"data":{"ok":true}}"#).await;
    let rec = recorder();
    let api = GraphqlApi::builder(server.uri())
        .max_parallel(1)
        .rate_limit(1000.0)
        .request_timeout(Duration::from_secs(5))
        .no_request_timeout()
        .observer("gql", rec.clone())
        .http_client(test_client())
        .build();
    let data = api.post_graphql("{ ok }", &Map::new()).await.unwrap();
    assert_eq!(data["ok"], json!(true));
    assert_eq!(rec.tags(), ["started", "ok:200"]);
}

#[tokio::test]
async fn the_configured_auth_header_travels() {
    let server = MockServer::start().await;
    mount_with_header(
        &server,
        "POST",
        ("Authorization", "Bearer t0k"),
        200,
        r#"{"data":{"ok":true}}"#,
    )
    .await;
    let api = GraphqlApi::builder(server.uri())
        .auth("Bearer t0k")
        .http_client(test_client())
        .build();
    let data = api.post_graphql("{ ok }", &Map::new()).await.unwrap();
    assert_eq!(data["ok"], json!(true));
}

#[tokio::test]
async fn an_auth_refresher_supplies_the_token_and_is_asked_again_on_401() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let server = MockServer::start().await;
    mount_with_header(&server, "POST", ("Authorization", "Bearer stale"), 401, "").await;
    mount_with_header(
        &server,
        "POST",
        ("Authorization", "Bearer fresh"),
        200,
        r#"{"data":{"ok":true}}"#,
    )
    .await;
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    let refresher: vantage_api_client::AuthRefresher = Arc::new(move || {
        let n = counter.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(if n == 0 { "stale" } else { "fresh" }.to_string()) })
    });
    let api = GraphqlApi::builder(server.uri())
        .auth_refresher(refresher)
        .http_client(test_client())
        .build();

    let data = api.post_graphql("{ ok }", &Map::new()).await.unwrap();
    assert_eq!(data["ok"], json!(true));
    assert_eq!(
        asked.load(Ordering::SeqCst),
        2,
        "once to start, once on the 401"
    );
}

#[tokio::test]
async fn a_fixed_auth_set_after_a_refresher_replaces_it() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let server = MockServer::start().await;
    mount_with_header(
        &server,
        "POST",
        ("Authorization", "Bearer fixed"),
        200,
        r#"{"data":{"ok":true}}"#,
    )
    .await;
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    let refresher: vantage_api_client::AuthRefresher = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok("refreshed".to_string()) })
    });
    let api = GraphqlApi::builder(server.uri())
        .auth_refresher(refresher)
        .auth("Bearer fixed")
        .http_client(test_client())
        .build();

    api.post_graphql("{ ok }", &Map::new()).await.unwrap();
    assert_eq!(asked.load(Ordering::SeqCst), 0, "the refresher is gone");
}

#[tokio::test]
async fn a_response_that_is_not_json_is_an_error() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, "<html>nope</html>").await;
    let api = GraphqlApi::builder(server.uri())
        .http_client(test_client())
        .build();
    let err = api.post_graphql("{ ok }", &Map::new()).await.unwrap_err();
    assert!(err.to_string().contains("JSON"), "{err}");
}

#[tokio::test]
async fn a_response_without_data_reads_as_null() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, "{}").await;
    let api = GraphqlApi::builder(server.uri())
        .http_client(test_client())
        .build();
    let data = api.post_graphql("{ ok }", &Map::new()).await.unwrap();
    assert!(data.is_null());
}

#[tokio::test]
async fn graphql_errors_are_joined_into_one_message() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        200,
        r#"{"data":null,"errors":[{"message":"first"},{"message":"second"}]}"#,
    )
    .await;
    let api = GraphqlApi::builder(server.uri())
        .http_client(test_client())
        .build();
    let err = Priority::Essential
        .scope(api.post_graphql("{ x }", &Map::new()))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("first; second"), "{err}");
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test]
async fn the_body_carries_the_query_and_variables() {
    use wiremock::matchers::{body_json, method};
    use wiremock::{Mock, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_json(json!({
            "query": "query ($limit: Int) { x(limit: $limit) { id } }",
            "variables": { "limit": 3 }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { "x": [] } })))
        .expect(1)
        .mount(&server)
        .await;
    let api = GraphqlApi::builder(server.uri())
        .http_client(test_client())
        .build();
    let mut variables = Map::new();
    variables.insert("limit".into(), json!(3));
    let data = api
        .post_graphql(
            "query ($limit: Int) { x(limit: $limit) { id } }",
            &variables,
        )
        .await
        .unwrap();
    assert_eq!(data["x"], json!([]));
}
