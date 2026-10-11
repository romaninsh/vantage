//! GraphQL safety: names outside the GraphQL `Name` grammar never reach a
//! request, writes are refused as unsupported, and listing keeps every row.

mod support;

use ciborium::Value as CborValue;
use serde_json::{Value, json};
use support::{mount, requests, test_client};
use vantage_api_client::{FilterDialect, GraphqlApi, GraphqlApiVistaSpec, GraphqlOperation};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::column::core::Column;
use vantage_table::sorting::{OrderBy, SortDirection};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::EmptyEntity;
use vantage_vista::VistaFactory;
use wiremock::MockServer;

const ROWS: &str = r#"{"data":{"launches":[]}}"#;

fn api(server: &MockServer) -> GraphqlApi {
    GraphqlApi::builder(server.uri())
        .http_client(test_client())
        .build()
}

fn launches(api: GraphqlApi) -> Table<GraphqlApi, EmptyEntity> {
    Table::new("launches", api)
        .with_id_column("id")
        .with_column_of::<String>("name")
}

/// The error from building a `launches` spec with `extra` YAML appended.
fn build_error(server: &MockServer, extra: &str) -> String {
    let yaml =
        format!("name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\n{extra}");
    let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(&yaml).unwrap();
    match api(server).vista_factory().build_from_spec(spec) {
        Ok(_) => panic!("spec with {extra:?} should not build"),
        Err(e) => e.to_string(),
    }
}

#[tokio::test]
async fn spec_names_are_checked_when_the_vista_is_built() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, ROWS).await;
    let cases = [
        (
            "graphql:\n  root_field: \"launches { secret }\"\n",
            "root field",
        ),
        ("graphql:\n  filter_arg: \"find)\"\n", "filter argument"),
        ("graphql:\n  count_field: \"a b\"\n", "count field"),
        ("graphql:\n  args: { \"in put\": {} }\n", "root argument"),
        (
            "graphql:\n  args: { input: { \"a}\": 1 } }\n",
            "root argument key",
        ),
        (
            "graphql:\n  response_path: \"edges.no de\"\n",
            "response path segment",
        ),
        ("  \"rocket name\":\n    type: string\n", "field"),
        ("  \"rocket.na-me\":\n    type: string\n", "field"),
    ];
    for (extra, kind) in cases {
        let err = build_error(&server, extra);
        assert!(err.contains("GraphQL name is not valid"), "{extra}: {err}");
        assert!(err.contains(kind), "{extra}: {err}");
    }
    assert_eq!(requests(&server).await, 0);
}

/// Names supplied at run time — conditions, order, the builder's overrides —
/// fail at render, before anything is posted.
#[tokio::test]
async fn runtime_names_fail_before_any_request() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, ROWS).await;

    let mut filtered = launches(api(&server));
    filtered.add_condition(Column::<vantage_api_client::AnyGraphqlType>::new("a b").eq("x"));
    let err = filtered.list_values().await.unwrap_err().to_string();
    assert!(err.contains("filter key"), "{err}");

    let hasura = GraphqlApi::builder(server.uri())
        .http_client(test_client())
        .dialect(FilterDialect::Hasura)
        .build();
    let mut ordered = launches(hasura);
    ordered.add_order(OrderBy {
        expression: Column::<vantage_api_client::AnyGraphqlType>::new("name: asc}, {x").eq("_"),
        direction: SortDirection::Ascending,
    });
    let err = ordered.list_values().await.unwrap_err().to_string();
    assert!(err.contains("order field"), "{err}");

    let bad_arg = GraphqlApi::builder(server.uri())
        .http_client(test_client())
        .filter_arg_name("find) { id } x: launches(find")
        .build();
    let mut table = launches(bad_arg);
    table.add_condition(Column::<vantage_api_client::AnyGraphqlType>::new("name").eq("x"));
    let err = table.list_values().await.unwrap_err().to_string();
    assert!(err.contains("filter argument"), "{err}");

    let bad_column = launches(api(&server)).with_column_of::<String>("name { secret }");
    let err = bad_column.list_values().await.unwrap_err().to_string();
    assert!(err.contains("field"), "{err}");

    assert_eq!(requests(&server).await, 0);
}

/// A runtime `count_field` is checked before the aggregate is posted.
#[tokio::test]
async fn an_invalid_count_field_fails_before_any_request() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, ROWS).await;
    let api = GraphqlApi::builder(server.uri())
        .http_client(test_client())
        .dialect(FilterDialect::Hasura)
        .count_field("users_aggregate { secret }")
        .build();
    let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(
        "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\ngraphql:\n  count: aggregate\n",
    )
    .unwrap();
    let vista = api.vista_factory().build_from_spec(spec).unwrap();
    let err = vista.get_count().await.unwrap_err().to_string();
    assert!(err.contains("count field"), "{err}");
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn writes_are_unsupported() {
    let server = MockServer::start().await;
    let api = api(&server);
    let table = launches(api.clone());
    let id = "1".to_string();
    let errors = [
        api.insert_table_value(&table, &id, &Default::default())
            .await
            .unwrap_err(),
        api.replace_table_value(&table, &id, &Default::default())
            .await
            .unwrap_err(),
        api.patch_table_value(&table, &id, &Default::default())
            .await
            .unwrap_err(),
        api.delete_table_value(&table, &id).await.unwrap_err(),
        api.delete_table_all_values(&table).await.unwrap_err(),
        api.insert_table_return_id_value(&table, &Default::default())
            .await
            .unwrap_err(),
    ];
    for err in errors {
        assert!(err.is_unsupported(), "{err}");
    }
    assert_eq!(requests(&server).await, 0);
}

async fn list(server: &MockServer, rows: Value) -> vantage_core::Result<usize> {
    let body = json!({ "data": { "launches": rows } }).to_string();
    mount(server, "POST", 200, &body).await;
    Ok(launches(api(server)).list_values().await?.len())
}

/// An id-less row keyed by its index, "1", would overwrite the real row
/// with that id; it gets a synthetic `~row:` key instead.
#[tokio::test]
async fn an_id_less_row_and_a_real_row_both_survive() {
    let server = MockServer::start().await;
    let rows = json!([{ "id": "1", "name": "real" }, { "id": null, "name": "no id" }]);
    assert_eq!(list(&server, rows).await.unwrap(), 2);
}

#[tokio::test]
async fn a_duplicate_id_is_an_error() {
    let server = MockServer::start().await;
    let rows = json!([{ "id": "7", "name": "a" }, { "id": "7", "name": "b" }]);
    let err = list(&server, rows).await.unwrap_err().to_string();
    assert!(err.contains("share an id"), "{err}");
    assert!(err.contains('7'), "{err}");
}

/// Without filter push-down, get-by-id searches the raw rows: an id-less row
/// and a pair of repeated ids elsewhere in the list don't get in its way.
#[tokio::test]
async fn get_by_id_reads_past_id_less_and_repeated_rows() {
    let server = MockServer::start().await;
    let rows = json!([
        { "id": null, "name": "no id" },
        { "id": "7", "name": "a" },
        { "id": "7", "name": "b" },
        { "id": "1", "name": "real" }
    ]);
    let body = json!({ "data": { "launches": rows } }).to_string();
    mount(&server, "POST", 200, &body).await;
    let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(
        "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\n  name:\n    type: string\ngraphql:\n  filter: false\n",
    )
    .unwrap();
    let vista = api(&server).vista_factory().build_from_spec(spec).unwrap();
    let row = vista.get_value("1").await.unwrap().expect("row 1");
    assert_eq!(row.get("name"), Some(&CborValue::Text("real".into())));
}
