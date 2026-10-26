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
    table
        .insert_value("d", &product("Delta", 40, "draft"))
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
async fn in_values_condition_narrows_to_the_listed_values() {
    use vantage_table::traits::table_source::TableSource;

    let (_tmp, table) = seeded_with_status_indexed().await;
    let db = table.data_source().clone();
    let listed = |ids: &[&str]| {
        let values = ids
            .iter()
            .map(|id| AnyRedbType::new(id.to_string()))
            .collect();
        table
            .clone()
            .with_condition(db.in_values_condition("id", values).unwrap())
    };
    assert_eq!(
        keys(&listed(&["a", "c"]).list_values().await.unwrap()),
        ["a", "c"]
    );
    assert!(listed(&[]).list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_in_on_indexed_column_unions_the_listed_values() {
    let (_tmp, table) = seeded_with_status_indexed().await;

    let mut q = table.clone();
    let status = q["status"].clone();
    q.add_condition(status.in_(vec!["active", "archived", "missing"]));

    assert_eq!(keys(&q.list_values().await.unwrap()), ["a", "b", "c"]);
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
async fn test_later_conditions_filter_the_rows_the_first_one_seeds() {
    let (_tmp, table) = seeded_with_status_indexed().await;
    let status = table["status"].clone();

    let mut by_id = table.clone();
    by_id.add_condition(status.eq("active"));
    by_id.add_condition(table["id"].clone().eq("b"));
    let mut by_name = table.clone();
    by_name.add_condition(status.eq("active"));
    by_name.add_condition(table["name"].clone().eq("Alpha"));

    assert_eq!(keys(&by_id.list_values().await.unwrap()), ["b"]);
    assert_eq!(keys(&by_name.list_values().await.unwrap()), ["a"]);
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
