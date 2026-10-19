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

#[tokio::test]
async fn removed_column_keeps_its_invariant() {
    let src = MockTableSource::new().with_data("client", vec![]).await;
    let mut table = MockTable::new("client", src)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("bakery_id")
        .with_invariant("bakery_id", json!(3));
    table.remove_column("bakery_id").unwrap();
    assert_eq!(table.invariants().get("bakery_id"), Some(&json!(3)));
    let id = table
        .insert_return_id_value(&Record::<Value>::from(json!({"name": "alice"})))
        .await
        .unwrap();
    let row = table.get_value(id).await.unwrap().unwrap();
    assert_eq!(row["bakery_id"], json!(3));
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
