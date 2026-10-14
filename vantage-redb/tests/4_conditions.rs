//! Test 4: Conditions on Table<Redb, EmptyEntity>.
//!
//! The first condition must be on an indexed column or the table's id
//! column; conditioning on an unflagged column first panics deliberately.
//! Later conditions are checked in memory and may name any column.

use vantage_dataset::prelude::*;
use vantage_redb::operation::RedbOperation;
use vantage_redb::{AnyRedbType, Redb};
use vantage_table::column::core::Column;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

fn product(name: &str, price: i64, status: &str) -> Record<AnyRedbType> {
    let mut r: Record<AnyRedbType> = Record::new();
    r.insert("name".into(), AnyRedbType::new(name.to_string()));
    r.insert("price".into(), AnyRedbType::new(price));
    r.insert("status".into(), AnyRedbType::new(status.to_string()));
    r
}

async fn seeded_with_status_indexed() -> (tempfile::NamedTempFile, Table<Redb, EmptyEntity>) {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let table = Table::<Redb, EmptyEntity>::new("product", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column(Column::<String>::new("status").with_flag(ColumnFlag::Indexed));

    table
        .insert_value("a", &product("Alpha", 10, "active"))
        .await
        .unwrap();
    table
        .insert_value("b", &product("Beta", 20, "active"))
        .await
        .unwrap();
    table
        .insert_value("c", &product("Gamma", 30, "archived"))
        .await
        .unwrap();
    (path, table)
}

async fn notes(bodies: &[&str]) -> (tempfile::NamedTempFile, Table<Redb, EmptyEntity>) {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let table = Table::<Redb, EmptyEntity>::new("notes", db)
        .with_id_column("id")
        .with_column_of::<String>("body");
    for body in bodies {
        let mut r: Record<AnyRedbType> = Record::new();
        r.insert("body".into(), AnyRedbType::new(body.to_string()));
        table.insert_value(*body, &r).await.unwrap();
    }
    (path, table)
}

fn keys(rows: &indexmap::IndexMap<String, Record<AnyRedbType>>) -> Vec<&str> {
    let mut keys: Vec<&str> = rows.keys().map(String::as_str).collect();
    keys.sort();
    keys
}

#[tokio::test]
async fn test_eq_on_indexed_column_returns_matches() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let status = q["status"].clone();
    q.add_condition(status.eq("active"));

    let rows = q.list_values().await.unwrap();
    assert_eq!(keys(&rows), ["a", "b"]);
}

#[tokio::test]
async fn test_eq_no_matches_returns_empty() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let status = q["status"].clone();
    q.add_condition(status.eq("nonexistent"));

    let rows = q.list_values().await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn test_in_on_indexed_column_unions_results() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let status = q["status"].clone();
    q.add_condition(status.in_(vec!["active", "archived"]));

    let rows = q.list_values().await.unwrap();
    assert_eq!(keys(&rows), ["a", "b", "c"]);
}

#[tokio::test]
async fn test_in_with_one_unknown_value() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let status = q["status"].clone();
    q.add_condition(status.in_(vec!["archived", "draft"]));

    let rows = q.list_values().await.unwrap();
    assert_eq!(keys(&rows), ["c"]);
}

#[tokio::test]
async fn test_in_with_no_values_returns_empty() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let status = q["status"].clone();
    q.add_condition(status.in_(Vec::<String>::new()));

    assert!(q.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_in_with_a_repeated_value_does_not_repeat_rows() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let status = q["status"].clone();
    q.add_condition(status.in_(vec!["archived", "archived"]));

    let rows = q.list_values().await.unwrap();
    assert_eq!(keys(&rows), ["c"]);
}

#[tokio::test]
async fn test_eq_on_an_indexed_integer_column() {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let table = Table::<Redb, EmptyEntity>::new("stock", db)
        .with_id_column("id")
        .with_column(Column::<i64>::new("qty").with_flag(ColumnFlag::Indexed));
    for (id, qty) in [("x", 5i64), ("y", 7), ("z", 5)] {
        let mut r: Record<AnyRedbType> = Record::new();
        r.insert("qty".into(), AnyRedbType::new(qty));
        table.insert_value(id, &r).await.unwrap();
    }

    let mut q = table.clone();
    let qty = q["qty"].clone();
    q.add_condition(qty.eq(5i64));

    assert_eq!(keys(&q.list_values().await.unwrap()), ["x", "z"]);
}

#[tokio::test]
async fn test_id_eq_short_circuits_without_index() {
    // No Indexed flag on any column — the id column is implicitly available.
    let (_tmp, table) = notes(&["n1", "n2"]).await;

    let mut q = table.clone();
    let id_col = q["id"].clone();
    q.add_condition(id_col.eq("n1"));

    let rows = q.list_values().await.unwrap();
    assert_eq!(keys(&rows), ["n1"]);
}

#[tokio::test]
async fn test_id_eq_missing_returns_empty() {
    let (_tmp, table) = notes(&["seed"]).await;

    let mut q = table.clone();
    let id_col = q["id"].clone();
    q.add_condition(id_col.eq("nonexistent"));

    let rows = q.list_values().await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn test_id_in_short_circuits() {
    let (_tmp, table) = notes(&["one", "two", "three"]).await;

    let mut q = table.clone();
    let id_col = q["id"].clone();
    q.add_condition(id_col.in_(vec!["one", "three", "missing"]));

    let rows = q.list_values().await.unwrap();
    assert_eq!(keys(&rows), ["one", "three"]);
}

#[tokio::test]
async fn test_multiple_conditions_first_seeds_rest_filter() {
    // First condition (status=active) seeds candidates via index;
    // second condition filters in memory by id.
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let status = q["status"].clone();
    let id_col = q["id"].clone();
    q.add_condition(status.eq("active"));
    q.add_condition(id_col.eq("b"));

    let rows = q.list_values().await.unwrap();
    assert_eq!(keys(&rows), ["b"]);
}

#[tokio::test]
async fn test_later_conditions_may_name_an_unindexed_column() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let status = q["status"].clone();
    let name = q["name"].clone();
    q.add_condition(status.eq("active"));
    q.add_condition(name.eq("Alpha"));

    let rows = q.list_values().await.unwrap();
    assert_eq!(keys(&rows), ["a"]);
}

#[tokio::test]
async fn test_conditions_that_exclude_each_other_return_empty() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let status = q["status"].clone();
    let id_col = q["id"].clone();
    q.add_condition(status.eq("archived"));
    q.add_condition(id_col.eq("a"));

    assert!(q.list_values().await.unwrap().is_empty());
}

#[tokio::test]
#[should_panic(expected = "non-indexed column")]
async fn test_eq_on_unflagged_column_panics() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    // `name` is not flagged Indexed and is not the id column.
    let name_col = q["name"].clone();
    q.add_condition(name_col.eq("Alpha"));
    let _ = q.list_values().await; // expected to panic
}

#[tokio::test]
#[should_panic(expected = "non-indexed column")]
async fn test_in_on_unflagged_column_panics() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let price_col = q["price"].clone();
    q.add_condition(price_col.in_(vec![10i64, 20]));
    let _ = q.list_values().await;
}
