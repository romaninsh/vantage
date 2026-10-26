//! GraphQL Vista paging and server-side counting, against a wiremock
//! endpoint. Each mock only answers a request whose body carries the
//! expected `query` / `variables`, so a wrong request surfaces as a 404
//! error from the fetch rather than as a silent mismatch.

mod support;

use ciborium::Value as CborValue;
use serde_json::{Value, json};
use vantage_api_client::{GraphqlApi, GraphqlApiVistaSpec};
use vantage_vista::{FilterOp, Vista, VistaFactory};
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

/// The window is fetched at its absolute offset and, a GraphQL list
/// reporting no total, carries none. An id-less row is keyed by its
/// absolute position in the result, so id-less rows from different pages
/// never share a key.
#[tokio::test]
async fn a_window_is_fetched_and_keyed_at_its_absolute_offset() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "variables": { "limit": 3, "offset": 10 } }),
        json!({ "data": { "launches": [
            { "id": "10", "name": "a" },
            { "id": null, "name": "b" },
            { "id": "12", "name": "c" }
        ] } }),
    )
    .await;
    let vista = vista(&server, "  paginate: true\n");

    let window = vista.fetch_window(10, 3).await.unwrap();
    let keys: Vec<&str> = window.rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(keys, ["10", "~row:11", "12"]);
    assert_eq!(window.total, None);
}

/// Offset paging is no continuation cursor, and with filtering done locally
/// a server page would come back short, so no window is offered even though
/// `limit`/`offset` would render. Both are refused without a request.
#[tokio::test]
async fn reads_the_vista_cannot_page_are_refused_without_a_request() {
    let server = MockServer::start().await;
    let paged = vista(&server, "  paginate: true\n");
    let locally_filtered = vista(&server, "  paginate: true\n  filter: false\n");

    let err = paged.fetch_cursor(None, 2).await.unwrap_err();
    assert!(err.is_unsupported(), "{err}");
    let err = locally_filtered.fetch_window(5, 10).await.unwrap_err();
    assert!(err.is_unsupported(), "{err}");
    assert_eq!(support::requests(&server).await, 0);
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

    assert_eq!(vista.fetch_window(0, 3).await.unwrap().rows.len(), 3);
}

/// Without paging, the leading window is cut from one unpaged listing: no
/// `limit`/`offset` is sent, and the page carries the listing's length.
#[tokio::test]
async fn the_leading_window_without_paging_is_cut_from_the_listing() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "query": "query { launches { id name } }" }),
        rows(0, 5),
    )
    .await;
    let vista = vista(&server, "  dialect: generic\n");

    let page = vista.fetch_window(0, 2).await.unwrap();
    let ids: Vec<&str> = page.rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["0", "1"]);
    assert_eq!(page.total, Some(5));
}

/// The preview's query names the endpoint the document is posted to, so
/// nothing is left for the note.
#[tokio::test]
async fn preview_posts_the_document_to_the_endpoint() {
    let server = MockServer::start().await;
    let preview = vista(&server, "  dialect: generic\n").preview();
    assert_eq!(
        preview.query.as_deref(),
        Some(format!("POST {}\nquery {{ launches {{ id name }} }}", server.uri()).as_str())
    );
    assert_eq!(preview.note, None);
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
    vista.set_search(Some("fal")).unwrap();

    assert_eq!(vista.count().await.unwrap(), 5);
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

    let err = vista.count().await.unwrap_err().to_string();
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
        .add_condition("name", FilterOp::Eq, CborValue::Text("FalconSat".into()))
        .unwrap();

    assert_eq!(vista.count().await.unwrap(), 42);
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

    assert_eq!(vista.count().await.unwrap(), 7);
}
