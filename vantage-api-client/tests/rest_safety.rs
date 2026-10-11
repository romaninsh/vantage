//! REST safety: bad headers are errors rather than panics, writes are
//! refused as unsupported, listing keeps every row, and a column's values
//! are read by listing.

mod support;

use ciborium::Value as CborValue;
use serde_json::json;
use support::{mount, requests, test_client};
use vantage_api_client::{ResponseShape, RestApi};
use vantage_dataset::traits::ReadableValueSet;
use vantage_expressions::ExprDataSource;
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::EmptyEntity;
use wiremock::MockServer;

fn api(server: &MockServer) -> RestApi {
    RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .http_client(test_client())
        .build()
}

fn users(api: RestApi) -> Table<RestApi, EmptyEntity> {
    Table::new("users", api).with_id_column("id")
}

#[tokio::test]
async fn bad_header_names_and_values_are_errors() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let api = api(&server);
    let get = reqwest::Method::GET;

    let err = api
        .http_request(get.clone(), "users", &[("bad name", "v")], None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("header name is not valid"), "{err}");

    let err = api
        .http_request(get.clone(), "users", &[("x-token", "a\r\nb")], None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("header value is not valid"), "{err}");
    assert!(
        !err.contains("a\r\nb"),
        "the value stays out of the error: {err}"
    );

    let bad_auth = RestApi::builder(server.uri())
        .auth("Bearer a\nb")
        .http_client(test_client())
        .build();
    let err = bad_auth
        .http_request(get, "users", &[], None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("Authorization"), "{err}");

    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn writes_are_unsupported() {
    let server = MockServer::start().await;
    let api = api(&server);
    let table = users(api.clone());
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

/// An id-less row keyed by its index, "1", would overwrite the real row
/// with that id; it gets a synthetic `~row:` key instead.
#[tokio::test]
async fn an_id_less_row_and_a_real_row_both_survive() {
    let server = MockServer::start().await;
    let rows = json!([{ "id": 1, "name": "real" }, { "name": "no id" }, { "id": null }]);
    mount(&server, "GET", 200, &rows.to_string()).await;
    let listed = users(api(&server)).list_values().await.unwrap();
    let keys: Vec<&str> = listed.keys().map(String::as_str).collect();
    assert_eq!(keys, ["1", "~row:1", "~row:2"]);
}

#[tokio::test]
async fn column_values_are_read_by_listing() {
    let server = MockServer::start().await;
    let rows = json!([{ "id": 1, "name": "a" }, { "id": 2 }, { "id": 3, "name": "c" }]);
    mount(&server, "GET", 200, &rows.to_string()).await;
    let api = api(&server);
    let table = users(api.clone());
    let column = Column::<CborValue>::new("name");
    let expr = api.column_table_values_expr(&table, &column);
    let values = api.execute(expr.expression()).await.unwrap();
    assert_eq!(
        values,
        CborValue::Array(vec![
            CborValue::Text("a".into()),
            CborValue::Text("c".into())
        ])
    );
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test]
async fn a_duplicate_id_is_an_error() {
    let server = MockServer::start().await;
    let rows = json!([{ "id": "7", "name": "a" }, { "id": 7, "name": "b" }]);
    mount(&server, "GET", 200, &rows.to_string()).await;
    let err = users(api(&server))
        .list_values()
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("share an id"), "{err}");
    assert!(err.contains('7'), "{err}");
}
