#![cfg(feature = "mocks")]

//! `Table::remove_column`: a removed column is neither selected nor shown, but
//! the table keeps narrowing and stamping by it.

use serde_json::{Value, json};
use vantage_dataset::prelude::*;
use vantage_expressions::Expressive;
use vantage_expressions::expr_any;
use vantage_expressions::mocks::datasource::MockSelectableDataSource;
use vantage_table::prelude::*;
use vantage_types::{EmptyEntity, Record};

type MockTable = Table<MockTableSource, EmptyEntity>;

fn client() -> MockTable {
    let src = MockTableSource::new().with_select_source(MockSelectableDataSource::new(json!([])));
    MockTable::new("client", src)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("bakery_id")
}

#[test]
fn removed_column_is_not_selected() {
    let mut table = client();
    table.remove_column("bakery_id").unwrap();
    assert_eq!(table.select().fields(), ["id", "name"]);
    assert!(!table.columns().contains_key("bakery_id"));
}

async fn stored_client() -> MockTable {
    let src = MockTableSource::new().with_data("client", vec![]).await;
    MockTable::new("client", src)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("bakery_id")
}

fn rec(value: Value) -> Record<Value> {
    Record::from(value)
}

#[tokio::test]
async fn removed_invariant_column_is_filled_not_refused() {
    let mut table = stored_client().await.with_invariant("bakery_id", json!(3));
    table.remove_column("bakery_id").unwrap();
    assert_eq!(table.invariants().get("bakery_id"), Some(&json!(3)));
    let id = table
        .insert_return_id_value(&rec(json!({"name": "alice"})))
        .await
        .unwrap();
    let row = table.get_value(id).await.unwrap().unwrap();
    assert_eq!(row["bakery_id"], json!(3));
    table
        .insert_value("9", &rec(json!({"name": "bob", "bakery_id": 3})))
        .await
        .unwrap();
    let err = table
        .insert_value("10", &rec(json!({"name": "eve", "bakery_id": 4})))
        .await
        .unwrap_err();
    assert!(err.is_forbidden(), "{err}");
}

#[tokio::test]
async fn a_removed_column_refuses_writes() {
    let src = MockTableSource::new()
        .with_data(
            "client",
            vec![json!({"id": "1", "name": "a", "bakery_id": 2})],
        )
        .await;
    let mut table = MockTable::new("client", src)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("bakery_id");
    table.remove_column("bakery_id").unwrap();
    let r = rec(json!({"name": "b", "bakery_id": 3}));
    let err = table.insert_value("9", &r).await.unwrap_err();
    assert!(err.is_forbidden(), "{err}");
    assert_eq!(
        err.context.get("column").map(String::as_str),
        Some("bakery_id")
    );
    let err = table.replace_value("1", &r).await.unwrap_err();
    assert!(err.is_forbidden(), "{err}");
    let err = table.patch_value("1", &r).await.unwrap_err();
    assert!(err.is_forbidden(), "{err}");
    let rows = table.list_values().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows["1"]["name"], json!("a"));
    assert_eq!(rows["1"]["bakery_id"], json!(2));
}

#[test]
fn removed_title_column_clears_the_title() {
    let mut table = client().with_title_column_of::<String>("nick");
    table.remove_column("nick").unwrap();
    assert!(table.title_field().is_none());
    assert!(table.title_fields().is_empty());
}

#[test]
fn removed_expr_column_is_not_selected() {
    let mut table = client().with_expression("shout", |_| expr_any!("upper(name)"));
    table.remove_column("shout").unwrap();
    let select = table.select();
    assert!(!select.expr().preview().contains("upper(name)"));
    assert!(!table.is_calculated_column("shout"));
}

#[test]
fn the_id_column_cannot_be_removed() {
    let mut table = client();
    let err = table.remove_column("id").unwrap_err();
    assert_eq!(
        err.message(),
        "`id` is the id column of `client`; it can't be removed"
    );
    assert!(table.columns().contains_key("id"));
}

#[test]
fn an_unknown_column_is_an_error() {
    let mut table = client();
    let err = table.remove_column("nope").unwrap_err();
    assert_eq!(err.message(), "no column `nope` in `client`");
}
