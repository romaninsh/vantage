//! GraphQL Vista ordering, quicksearch, "contains" and get-by-id, against a
//! wiremock endpoint. Each mock only answers a request whose body carries
//! the expected `query` / `variables`, so a wrong request surfaces as an
//! error from the fetch rather than as a silent mismatch.

mod support;

use ciborium::Value as CborValue;
use serde_json::{Value, json};
use vantage_api_client::{GraphqlApi, GraphqlApiVistaSpec};
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::{ColumnFlag, FilterOp, SortDirection, Vista, VistaFactory};
use wiremock::matchers::{body_partial_json, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A `launches` Vista with `columns` (YAML, indented two spaces) and
/// `graphql` lines appended to its `graphql:` block.
fn vista(server: &MockServer, columns: &str, graphql: &str) -> Vista {
    let yaml = format!("name: launches\ncolumns:\n{columns}graphql:\n{graphql}");
    let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(&yaml).unwrap();
    GraphqlApi::builder(server.uri())
        .http_client(support::test_client())
        .build()
        .vista_factory()
        .build_from_spec(spec)
        .unwrap()
}

const ID_AND_NAME: &str = "  id:\n    type: string\n    flags: [id]\n  name:\n    type: string\n";

fn launches(rows: Value) -> Value {
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
async fn set_order_reaches_the_request() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "query": "query { launches(order_by: [{name: desc}]) { id name } }" }),
        launches(json!([{ "id": "1", "name": "b" }, { "id": "2", "name": "a" }])),
    )
    .await;
    let mut vista = vista(&server, ID_AND_NAME, "  dialect: hasura\n");
    vista
        .set_order(Some(("name", SortDirection::Descending)))
        .unwrap();

    assert_eq!(vista.list_values().await.unwrap().len(), 2);
}

/// Hasura spells a nested order as a nested object, which isn't rendered,
/// so a dotted column is not offered for ordering.
#[tokio::test]
async fn ordering_a_dotted_column_is_refused() {
    let server = MockServer::start().await;
    let columns = format!("{ID_AND_NAME}  rocket.name:\n    type: string\n");
    let mut vista = vista(&server, &columns, "  dialect: hasura\n");

    assert!(
        vista
            .column("name")
            .unwrap()
            .has_flag(ColumnFlag::Orderable)
    );
    assert!(
        !vista
            .column("rocket.name")
            .unwrap()
            .has_flag(ColumnFlag::Orderable)
    );
    assert!(
        vista
            .set_order(Some(("rocket.name", SortDirection::Ascending)))
            .is_err()
    );
}

/// Only string-typed, non-id, non-dotted columns take part in a search.
#[tokio::test]
async fn hasura_search_covers_string_columns_only() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({
            "query": "query { launches(where: {_or: [{name: {_ilike: \"%fal%\"}}, {details: {_ilike: \"%fal%\"}}]}) { id name year details rocket { name } } }"
        }),
        launches(json!([{ "id": 1, "name": "Falcon", "year": 2006, "details": null, "rocket": { "name": "F1" } }])),
    )
    .await;
    let columns = "  id:\n    type: int\n    flags: [id]\n  name:\n    type: string\n  year:\n    type: int\n  details:\n    type: string\n  rocket.name:\n    type: string\n";
    let mut vista = vista(&server, columns, "  dialect: hasura\n");
    vista.set_search(Some("fal")).unwrap();

    assert_eq!(vista.list_values().await.unwrap().len(), 1);
}

/// With nothing to search in, a search matches no row — and does not fail.
#[tokio::test]
async fn search_without_string_columns_matches_nothing() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "query": "query { launches(where: {_not: {}}) { id year } }" }),
        launches(json!([])),
    )
    .await;
    let columns = "  id:\n    type: int\n    flags: [id]\n  year:\n    type: int\n";
    let mut vista = vista(&server, columns, "  dialect: hasura\n");
    vista.set_search(Some("fal")).unwrap();

    assert!(vista.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn contains_renders_ilike_on_hasura() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "query": "query { launches(where: {name: {_ilike: \"%fal%\"}}) { id name } }" }),
        launches(json!([{ "id": "1", "name": "Falcon" }])),
    )
    .await;
    let mut vista = vista(&server, ID_AND_NAME, "  dialect: hasura\n");
    vista
        .add_condition("name", FilterOp::Like, CborValue::Text("fal".into()))
        .unwrap();

    assert_eq!(vista.list_values().await.unwrap().len(), 1);
}

#[tokio::test]
async fn contains_is_refused_on_generic() {
    let server = MockServer::start().await;
    let mut vista = vista(&server, ID_AND_NAME, "  dialect: generic\n");
    let err = vista
        .add_condition("name", FilterOp::Like, CborValue::Text("fal".into()))
        .unwrap_err();
    assert!(err.is_unsupported(), "{err}");
    assert!(vista.query().conditions.is_empty());
}

/// An int id column filters with an integer literal; with paging off no
/// `limit` is sent.
#[tokio::test]
async fn get_by_int_id_sends_an_integer_and_no_limit() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "query": "query { launches(where: {id: {_eq: 42}}) { id name } }" }),
        launches(json!([{ "id": 42, "name": "Falcon" }])),
    )
    .await;
    let columns = "  id:\n    type: int\n    flags: [id]\n  name:\n    type: string\n";
    let vista = vista(&server, columns, "  dialect: hasura\n");

    let row = vista.get_value("42").await.unwrap().expect("row");
    assert_eq!(row.get("name"), Some(&CborValue::Text("Falcon".into())));
}

/// A string id is quoted; with paging on the fetch is capped at one row.
#[tokio::test]
async fn get_by_string_id_quotes_it_and_limits_with_paging_on() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({
            "query": "query ($limit: Int) { launches(where: {id: {_eq: \"7\"}}, limit: $limit) { id name } }",
            "variables": { "limit": 1 }
        }),
        launches(json!([{ "id": "7", "name": "Falcon" }])),
    )
    .await;
    let vista = vista(
        &server,
        ID_AND_NAME,
        "  dialect: hasura\n  paginate: true\n",
    );

    assert!(vista.get_value("7").await.unwrap().is_some());
}

/// A root field taking no filter is listed whole and the row picked out
/// locally by its id column: sending the id would be rejected, a limit
/// would cut the list before the row is reached, a repeated id before the
/// row doesn't get in the way, and an id-less row's position key must not
/// stand in for a real id.
#[tokio::test]
async fn get_by_id_without_filter_push_down_lists_and_picks() {
    let server = MockServer::start().await;
    answer(
        &server,
        json!({ "query": "query { launches { id name } }" }),
        launches(json!([
            { "id": "7", "name": "a" },
            { "id": "7", "name": "b" },
            { "id": "3", "name": "right" },
            { "id": null, "name": "wrong" }
        ])),
    )
    .await;
    let vista = vista(
        &server,
        ID_AND_NAME,
        "  dialect: hasura\n  filter: false\n  paginate: true\n",
    );

    let row = vista.get_value("3").await.unwrap().expect("row");
    assert_eq!(row.get("name"), Some(&CborValue::Text("right".into())));
    assert!(vista.get_value("9").await.unwrap().is_none());
}

#[tokio::test]
async fn get_by_non_integer_id_on_an_int_column_is_an_error() {
    let server = MockServer::start().await;
    let columns = "  id:\n    type: int\n    flags: [id]\n  name:\n    type: string\n";
    let vista = vista(&server, columns, "  dialect: hasura\n");

    let err = vista.get_value("abc").await.unwrap_err().to_string();
    assert!(err.contains("Id is not an integer"), "{err}");
    assert_eq!(support::requests(&server).await, 0);
}
