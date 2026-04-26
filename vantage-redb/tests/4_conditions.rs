//! Test 4: Conditions on Table<Redb, EmptyEntity>.
//!
//! Conditions only fire on indexed columns or the table's id column;
//! conditioning on an unflagged column panics deliberately.

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
        .insert_value(&"a".to_string(), &product("Alpha", 10, "active"))
        .await
        .unwrap();
    table
        .insert_value(&"b".to_string(), &product("Beta", 20, "active"))
        .await
        .unwrap();
    table
        .insert_value(&"c".to_string(), &product("Gamma", 30, "archived"))
        .await
        .unwrap();
    (path, table)
}

#[tokio::test]
async fn test_eq_on_indexed_column_returns_matches() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let status = q["status"].clone();
    q.add_condition(status.eq("active"));

    let rows = q.list_values().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("a"));
    assert!(rows.contains_key("b"));
    assert!(!rows.contains_key("c"));
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
    assert_eq!(rows.len(), 3);
}

#[tokio::test]
async fn test_in_with_one_unknown_value() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let status = q["status"].clone();
    q.add_condition(status.in_(vec!["archived", "draft"]));

    let rows = q.list_values().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows.contains_key("c"));
}

#[tokio::test]
async fn test_id_eq_short_circuits_without_index() {
    // No Indexed flag on any column — the id column is implicitly available.
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let table = Table::<Redb, EmptyEntity>::new("notes", db)
        .with_id_column("id")
        .with_column_of::<String>("body");

    let mut r: Record<AnyRedbType> = Record::new();
    r.insert("body".into(), AnyRedbType::new("hi".to_string()));
    table.insert_value(&"n1".to_string(), &r).await.unwrap();

    let mut q = table.clone();
    let id_col = q["id"].clone();
    q.add_condition(id_col.eq("n1"));

    let rows = q.list_values().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows.contains_key("n1"));
}

#[tokio::test]
async fn test_id_eq_missing_returns_empty() {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let table = Table::<Redb, EmptyEntity>::new("notes", db)
        .with_id_column("id")
        .with_column_of::<String>("body");

    // Insert and remove to force the table to be created.
    let mut r: Record<AnyRedbType> = Record::new();
    r.insert("body".into(), AnyRedbType::new("seed".to_string()));
    table.insert_value(&"seed".to_string(), &r).await.unwrap();

    let mut q = table.clone();
    let id_col = q["id"].clone();
    q.add_condition(id_col.eq("nonexistent"));

    let rows = q.list_values().await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn test_id_in_short_circuits() {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let table = Table::<Redb, EmptyEntity>::new("notes", db)
        .with_id_column("id")
        .with_column_of::<String>("body");

    for n in ["one", "two", "three"] {
        let mut r: Record<AnyRedbType> = Record::new();
        r.insert("body".into(), AnyRedbType::new(n.to_string()));
        table.insert_value(&n.to_string(), &r).await.unwrap();
    }

    let mut q = table.clone();
    let id_col = q["id"].clone();
    q.add_condition(id_col.in_(vec!["one", "three", "missing"]));

    let rows = q.list_values().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("one"));
    assert!(rows.contains_key("three"));
    assert!(!rows.contains_key("two"));
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
    assert_eq!(rows.len(), 1);
    assert!(rows.contains_key("b"));
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
