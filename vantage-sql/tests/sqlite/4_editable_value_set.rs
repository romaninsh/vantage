//! Test 4: WritableValueSet and InsertableValueSet for Table<SqliteDB, Entity>.
//!
//! Uses in-memory SQLite — no entity deserialization, operates on raw Records.

#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use vantage_dataset::{InsertableValueSet, ReadableValueSet, WritableValueSet};

async fn setup() -> (SqliteDB, Table<SqliteDB, EmptyEntity>) {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    sqlx::query(
        "CREATE TABLE item (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            price INTEGER NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query("INSERT INTO item VALUES ('a', 'Alpha', 10), ('b', 'Beta', 20)")
        .execute(db.pool())
        .await
        .unwrap();

    let table = Table::<SqliteDB, EmptyEntity>::new("item", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");

    (db, table)
}

fn record(fields: &[(&str, AnySqliteType)]) -> Record<AnySqliteType> {
    fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

// ── WritableValueSet ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_insert_value() {
    let (_db, table) = setup().await;

    let rec = record(&[("name", "Gamma".into()), ("price", 30i64.into())]);
    let result = table.insert_value(&"c".to_string(), &rec).await.unwrap();
    assert_eq!(result["name"].try_get::<String>().unwrap(), "Gamma");

    // Verify it's actually there
    let fetched = table.get_value(&"c".to_string()).await.unwrap();
    assert_eq!(fetched["price"].try_get::<i64>().unwrap(), 30);
}

#[tokio::test]
async fn test_replace_value() {
    let (_db, table) = setup().await;

    let rec = record(&[("name", "Alpha Replaced".into()), ("price", 99i64.into())]);
    table.replace_value(&"a".to_string(), &rec).await.unwrap();

    let fetched = table.get_value(&"a".to_string()).await.unwrap();
    assert_eq!(
        fetched["name"].try_get::<String>().unwrap(),
        "Alpha Replaced"
    );
    assert_eq!(fetched["price"].try_get::<i64>().unwrap(), 99);
}

#[tokio::test]
async fn test_patch_value() {
    let (_db, table) = setup().await;

    // Patch only the price
    let partial = record(&[("price", 55i64.into())]);
    table.patch_value(&"a".to_string(), &partial).await.unwrap();

    let fetched = table.get_value(&"a".to_string()).await.unwrap();
    assert_eq!(fetched["name"].try_get::<String>().unwrap(), "Alpha"); // unchanged
    assert_eq!(fetched["price"].try_get::<i64>().unwrap(), 55); // updated
}

#[tokio::test]
async fn test_delete() {
    let (_db, table) = setup().await;

    table.delete(&"a".to_string()).await.unwrap();

    let all = table.list_values().await.unwrap();
    assert_eq!(all.len(), 1);
    assert!(!all.contains_key("a"));
}

#[tokio::test]
async fn test_delete_all() {
    let (_db, table) = setup().await;

    table.delete_all().await.unwrap();

    let all = table.list_values().await.unwrap();
    assert!(all.is_empty());
}

// ── InsertableValueSet ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_insert_return_id_value() {
    let (db, _) = setup().await;

    // Use a table with INTEGER PRIMARY KEY AUTOINCREMENT for auto-generated IDs
    sqlx::query(
        "CREATE TABLE log_entry (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            message TEXT NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    let table = Table::<SqliteDB, EmptyEntity>::new("log_entry", db)
        .with_id_column("id")
        .with_column_of::<String>("message");

    let rec = record(&[("message", "hello world".into())]);
    let id = table.insert_return_id_value(&rec).await.unwrap();
    assert!(!id.is_empty());

    let fetched = table.get_value(&id).await.unwrap();
    assert_eq!(
        fetched["message"].try_get::<String>().unwrap(),
        "hello world"
    );
}
