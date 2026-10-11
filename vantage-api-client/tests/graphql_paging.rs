//! GraphQL Vista paging and server-side counting, against a wiremock
//! endpoint. Each mock only answers a request whose body carries the
//! expected `query` / `variables`, so a wrong request surfaces as a 404
//! error from the fetch rather than as a silent mismatch.

mod support;

use ciborium::Value as CborValue;
use serde_json::{Value, json};
use vantage_api_client::{GraphqlApi, GraphqlApiVistaSpec};
use vantage_vista::{Vista, VistaFactory};
use wiremock::matchers::{body_partial_json, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A `launches` Vista with `id` + `name`, plus `extra` lines appended to
/// its `graphql:` block.
fn vista(server: &MockServer, extra: &str) -> Vista {
    let yaml = format!(
        "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\n  name:\n    type: string\ngraphql:\n{extra}"
    );
    let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(&yaml).unwrap();
    GraphqlApi::builder(server.uri())
        .http_client(support::test_client())
        .build()
        .vista_factory()
        .build_from_spec(spec)
        .unwrap()
}

/// `n` rows with ids starting at `first`.
fn rows(first: usize, n: usize) -> Value {
    let rows: Vec<Value> = (first..first + n)
        .map(|i| json!({ "id": i.to_string(), "name": format!("launch {i}") }))
        .collect();
    json!({ "data": { "launches": rows } })
}

/// Answer only requests whose JSON body contains `expected`.
async fn answer(server: &MockServer, expected: Value, response: Value) {
    Mock::given(method("POST"))
        .and(body_partial_json(expected))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .mount(server)
        .await;
}

#[tokio::test]
async fn fetch_page_sends_limit_and_offset_for_the_page() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "variables": { "limit": 10, "offset": 10 } }),
        rows(10, 10),
    )
    .await;
    let mut vista = vista(&server, "  paginate: true\n");
    vista.set_page_size(10).unwrap();

    let page = vista.fetch_page(2).await.unwrap();
    assert_eq!(page.len(), 10);
    assert_eq!(page[0].0, "10");
}

#[tokio::test]
async fn fetch_window_sends_the_absolute_offset() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "variables": { "limit": 7, "offset": 35 } }),
        rows(35, 7),
    )
    .await;
    let vista = vista(&server, "  paginate: true\n");

    let window = vista.fetch_window(35, 7).await.unwrap();
    assert_eq!(window.len(), 7);
    assert_eq!(window[0].0, "35");
}

#[tokio::test]
async fn fetch_next_chain_ends_on_a_short_page() {
    let server = MockServer::start().await;
    for (offset, n) in [(0, 2), (2, 2), (4, 1)] {
        answer(
            &server,
            json!({ "variables": { "limit": 2, "offset": offset } }),
            rows(offset, n),
        )
        .await;
    }
    let mut vista = vista(&server, "  paginate: true\n");
    vista.set_page_size(2).unwrap();

    let mut token = None;
    let mut ids = Vec::new();
    let mut tokens = Vec::new();
    loop {
        let (page, next) = vista.fetch_next(token).await.unwrap();
        ids.extend(page.into_iter().map(|(id, _)| id));
        tokens.push(next.clone());
        match next {
            Some(t) => token = Some(t),
            None => break,
        }
    }
    assert_eq!(ids, ["0", "1", "2", "3", "4"]);
    assert_eq!(
        tokens,
        [
            Some(CborValue::Integer(2.into())),
            Some(CborValue::Integer(3.into())),
            None
        ]
    );
}

/// Offset paging needs a stable order; Hasura can spell one, so a paged
/// read with no consumer order is ordered by the id column.
#[tokio::test]
async fn hasura_paged_read_orders_by_id_when_no_order_is_set() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({
            "query": "query ($limit: Int, $offset: Int) { launches(order_by: [{id: asc}], limit: $limit, offset: $offset) { id name } }"
        }),
        rows(0, 3),
    )
    .await;
    let vista = vista(&server, "  dialect: hasura\n  paginate: true\n");

    assert_eq!(vista.fetch_window(0, 3).await.unwrap().len(), 3);
}

#[tokio::test]
async fn paging_capabilities_follow_paginate() {
    let server = MockServer::start().await;
    let caps = vista(&server, "  paginate: true\n").capabilities().clone();
    assert!(caps.can_set_page_size);
    assert!(caps.can_fetch_page);
    assert!(caps.can_fetch_window);
    assert!(caps.can_fetch_next);

    let mut off = vista(&server, "  dialect: generic\n");
    let caps = off.capabilities().clone();
    assert!(!caps.can_set_page_size);
    assert!(!caps.can_fetch_page);
    assert!(!caps.can_fetch_window);
    assert!(!caps.can_fetch_next);
    assert!(off.set_page_size(10).is_err());
    assert!(off.fetch_window(0, 10).await.is_err());
}

/// With filtering done locally a server page would come back short, so
/// paging is not offered even though `limit`/`offset` would render.
#[tokio::test]
async fn paging_is_not_advertised_with_filter_false() {
    let server = MockServer::start().await;
    let mut vista = vista(&server, "  paginate: true\n  filter: false\n");
    let caps = vista.capabilities().clone();
    assert!(!caps.can_set_page_size);
    assert!(!caps.can_fetch_page);
    assert!(!caps.can_fetch_window);
    assert!(!caps.can_fetch_next);
    assert!(vista.set_page_size(10).is_err());
    assert!(vista.fetch_window(0, 10).await.is_err());
    assert_eq!(support::requests(&server).await, 0);
}

/// Without `count: aggregate`, Hasura counts by listing — a role lacking
/// `allow_aggregations` still gets an answer.
#[tokio::test]
async fn hasura_count_lists_by_default() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "query": "query { launches { id name } }" }),
        rows(0, 4),
    )
    .await;
    let vista = vista(&server, "  dialect: hasura\n");

    assert_eq!(vista.get_count().await.unwrap(), 4);
}

#[tokio::test]
async fn hasura_aggregate_count_applies_the_search() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({
            "query": "query { launches_aggregate(where: {name: {_ilike: \"%fal%\"}}) { aggregate { count } } }"
        }),
        json!({ "data": { "launches_aggregate": { "aggregate": { "count": 5 } } } }),
    )
    .await;
    let mut vista = vista(&server, "  dialect: hasura\n  count: aggregate\n");
    vista.add_search("fal").unwrap();

    assert_eq!(vista.get_count().await.unwrap(), 5);
}

/// A server refusing the aggregate fails the count, with the fix named,
/// rather than quietly listing instead.
#[tokio::test]
async fn rejected_aggregate_count_names_count_list() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "query": "query { launches_aggregate { aggregate { count } } }" }),
        json!({ "errors": [{ "message": "field 'launches_aggregate' not found in type: 'query_root'" }] }),
    )
    .await;
    let vista = vista(&server, "  dialect: hasura\n  count: aggregate\n");

    let err = vista.get_count().await.unwrap_err().to_string();
    assert!(err.contains("count: list"), "{err}");
    assert!(err.contains("launches_aggregate"), "{err}");
    assert_eq!(support::requests(&server).await, 1, "no fallback listing");
}

#[test]
fn an_unknown_count_mode_is_a_yaml_error() {
    let yaml = "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\ngraphql:\n  count: exact\n";
    assert!(serde_yaml_ng::from_str::<GraphqlApiVistaSpec>(yaml).is_err());
}

#[tokio::test]
async fn hasura_count_runs_the_aggregate_query_with_the_filter() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({
            "query": "query { launches_aggregate(where: {name: {_eq: \"FalconSat\"}}) { aggregate { count } } }"
        }),
        json!({ "data": { "launches_aggregate": { "aggregate": { "count": 42 } } } }),
    )
    .await;
    let mut vista = vista(&server, "  dialect: hasura\n  count: aggregate\n");
    vista
        .add_condition_eq("name", CborValue::Text("FalconSat".into()))
        .unwrap();

    assert_eq!(vista.get_count().await.unwrap(), 42);
}

#[tokio::test]
async fn hasura_count_field_is_overridable() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "query": "query { launchesStats { aggregate { count } } }" }),
        json!({ "data": { "launchesStats": { "aggregate": { "count": 7 } } } }),
    )
    .await;
    let vista = vista(
        &server,
        "  dialect: hasura\n  count: aggregate\n  count_field: launchesStats\n",
    );

    assert_eq!(vista.get_count().await.unwrap(), 7);
}

#[tokio::test]
async fn generic_count_still_lists_and_counts() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "query": "query { launches { id name } }" }),
        rows(0, 3),
    )
    .await;
    let vista = vista(&server, "  dialect: generic\n");

    assert_eq!(vista.get_count().await.unwrap(), 3);
}
