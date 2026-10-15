//! Columns that read a server field of another name (`graphql: { field }`),
//! against a wiremock endpoint. The query selects the field under the
//! column's name, so rows carry column names; conditions, order and id
//! lookups are sent as the server field.

mod support;

use ciborium::Value as CborValue;
use serde_json::{Value, json};
use vantage_api_client::{GraphqlApi, GraphqlApiVistaSpec};
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::{FilterOp, SortDirection, Vista, VistaFactory};
use wiremock::matchers::{body_partial_json, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A Hasura `launches` Vista with `columns` (YAML, indented two spaces) and
/// `graphql` lines appended to its `graphql:` block.
fn vista(server: &MockServer, columns: &str, graphql: &str) -> Vista {
    try_vista(server, columns, graphql).unwrap()
}

fn try_vista(server: &MockServer, columns: &str, graphql: &str) -> vantage_core::Result<Vista> {
    let yaml = format!("name: launches\ncolumns:\n{columns}graphql:\n  dialect: hasura\n{graphql}");
    let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(&yaml).unwrap();
    GraphqlApi::builder(server.uri())
        .http_client(support::test_client())
        .build()
        .vista_factory()
        .build_from_spec(spec)
}

/// `id` plus a `mission` column reading the `mission_name` field.
const MISSION: &str = "  id:\n    type: string\n    flags: [id]\n  mission:\n    type: string\n    graphql:\n      field: mission_name\n";

/// Answer only requests whose JSON body contains `query`, with `rows`.
async fn answer(server: &MockServer, query: &str, rows: Value) {
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "query": query })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "data": { "launches": rows } })),
        )
        .mount(server)
        .await;
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

#[tokio::test]
async fn filter_and_order_are_sent_as_the_server_field() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches(where: {mission_name: {_eq: \"Falcon\"}}, order_by: [{mission_name: asc}]) { id mission: mission_name } }",
        json!([{ "id": "1", "mission": "Falcon" }]),
    )
    .await;
    let mut vista = vista(&server, MISSION, "");
    vista
        .add_condition("mission", FilterOp::Eq, text("Falcon"))
        .unwrap();
    vista
        .set_order(Some(("mission", SortDirection::Ascending)))
        .unwrap();

    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows["1"].get("mission"), Some(&text("Falcon")));
}

/// Without filter push-down the condition stays off the wire and narrows
/// the aliased rows by column name.
#[tokio::test]
async fn without_filter_push_down_the_rows_are_filtered_by_column_name() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches { id mission: mission_name } }",
        json!([
            { "id": "1", "mission": "Falcon" },
            { "id": "2", "mission": "Starship" }
        ]),
    )
    .await;
    let mut vista = vista(&server, MISSION, "  filter: false\n");
    vista
        .add_condition("mission", FilterOp::Eq, text("Falcon"))
        .unwrap();

    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows["1"].get("mission"), Some(&text("Falcon")));
}

/// An aliased id column looks a row up by the server field and reads it
/// back under the column name.
#[tokio::test]
async fn get_by_id_on_an_aliased_id_column_filters_on_the_server_field() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches(where: {uuid: {_eq: \"7\"}}) { key: uuid name } }",
        json!([{ "key": "7", "name": "Falcon" }]),
    )
    .await;
    let columns = "  key:\n    type: string\n    flags: [id]\n    graphql:\n      field: uuid\n  name:\n    type: string\n";
    let vista = vista(&server, columns, "");

    let row = vista.get_value("7").await.unwrap().expect("row 7");
    assert_eq!(row.get("name"), Some(&text("Falcon")));
}

/// The aggregate count sees the same narrowing a list would, so its filter
/// names the server field too.
#[tokio::test]
async fn the_aggregate_count_filters_on_the_server_field() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({
            "query": "query { launches_aggregate(where: {mission_name: {_eq: \"x\"}}) { aggregate { count } } }"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({ "data": { "launches_aggregate": { "aggregate": { "count": 4 } } } }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let mut vista = vista(&server, MISSION, "  count: aggregate\n");
    vista
        .add_condition("mission", FilterOp::Eq, text("x"))
        .unwrap();

    assert_eq!(vista.count().await.unwrap(), 4);
}

/// A quicksearch over an aliased column is sent as the server field.
#[tokio::test]
async fn search_on_an_aliased_column_is_sent_as_the_server_field() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches(where: {_or: [{mission_name: {_ilike: \"%fal%\"}}, {details: {_ilike: \"%fal%\"}}]}) { id mission: mission_name details } }",
        json!([{ "id": "1", "mission": "Falcon", "details": null }]),
    )
    .await;
    let columns = "  id:\n    type: string\n    flags: [id]\n  mission:\n    type: string\n    flags: [searchable]\n    graphql:\n      field: mission_name\n  details:\n    type: string\n    flags: [searchable]\n";
    let mut vista = vista(&server, columns, "");
    vista.set_search(Some("fal")).unwrap();

    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows["1"].get("mission"), Some(&text("Falcon")));
}

#[tokio::test]
async fn aliases_must_be_plain_names() {
    let server = MockServer::start().await;
    let dotted_field = "  id:\n    type: string\n    flags: [id]\n  run_id:\n    type: string\n    graphql:\n      field: run.id\n";
    let err = try_vista(&server, dotted_field, "").expect_err("refused");
    assert!(err.is_unsupported(), "{err}");
    assert!(err.to_string().contains("dotted"), "{err}");

    let dotted_column = "  id:\n    type: string\n    flags: [id]\n  run.id:\n    type: string\n    graphql:\n      field: run_id\n";
    let err = try_vista(&server, dotted_column, "").expect_err("refused");
    assert!(err.is_unsupported(), "{err}");

    let bad_field = "  id:\n    type: string\n    flags: [id]\n  run_id:\n    type: string\n    graphql:\n      field: \"run id\"\n";
    let err = try_vista(&server, bad_field, "").expect_err("refused");
    assert!(
        err.to_string().contains("GraphQL name is not valid"),
        "{err}"
    );

    let bad_column = "  id:\n    type: string\n    flags: [id]\n  \"run id\":\n    type: string\n    graphql:\n      field: run_id\n";
    let err = try_vista(&server, bad_column, "").expect_err("refused");
    assert!(
        err.to_string().contains("GraphQL name is not valid"),
        "{err}"
    );
    assert_eq!(support::requests(&server).await, 0);
}
