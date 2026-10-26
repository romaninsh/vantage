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
async fn test_patch_value() {
    let (_db, table) = setup().await;

    // Patch only the price
    let partial = record(&[("price", 55i64.into())]);
    table.patch_value("a", &partial).await.unwrap();

    let fetched = table.get_value("a").await.unwrap().expect("a exists");
    assert_eq!(fetched["name"].try_get::<String>().unwrap(), "Alpha"); // unchanged
    assert_eq!(fetched["price"].try_get::<i64>().unwrap(), 55); // updated
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

    let fetched = table.get_value(&id).await.unwrap().expect("inserted row");
    assert_eq!(
        fetched["message"].try_get::<String>().unwrap(),
        "hello world"
    );
}

// ── Conditioned deletes ────────────────────────────────────────────────────

/// A conditioned table represents a SUBSET, so `delete_all` must delete that
/// subset and nothing else. Reported from the field as total data loss: the
/// condition was added, the delete ran, and every row went.
#[tokio::test]
async fn delete_all_on_a_conditioned_table_only_deletes_matching_rows() {
    let (_db, mut table) = setup().await;
    table.add_condition(vantage_sql::sqlite_expr!(
        "{} = {}",
        (table["name"]),
        "Alpha"
    ));

    table.delete_all().await.unwrap();

    // The condition names one row. The other must survive.
    let survivors = Table::<SqliteDB, EmptyEntity>::new("item", _db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .list_values()
        .await
        .unwrap();
    assert!(
        survivors.contains_key("b"),
        "row 'b' did not match the condition but was deleted anyway — \
         delete_all ignored the table's conditions and truncated the table",
    );
    assert_eq!(survivors.len(), 1, "only the matching row should have gone");
}
