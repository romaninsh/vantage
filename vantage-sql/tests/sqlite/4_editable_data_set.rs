//! Test 4: WritableDataSet and InsertableDataSet for Table<SqliteDB, Entity>.
//!
//! Uses in-memory SQLite with a typed entity.

#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::prelude::IdGenerator;
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::{InsertableDataSet, ReadableDataSet, WritableDataSet, WritableValueSet};

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Item {
    name: String,
    price: i64,
}

impl Item {
    fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Item> {
        Table::new("item", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("price")
    }
}

async fn setup() -> (SqliteDB, Table<SqliteDB, Item>) {
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

    let table = Item::sqlite_table(db.clone());
    (db, table)
}

// ── WritableDataSet ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_replace() {
    let (_db, table) = setup().await;

    let item = Item {
        name: "Alpha Replaced".into(),
        price: 99,
    };
    table.replace("a", &item).await.unwrap();

    let fetched = table.get("a").await.unwrap().expect("row a");
    assert_eq!(fetched, item);
}

#[tokio::test]
async fn test_delete() {
    let (_db, table) = setup().await;

    table.delete("a").await.unwrap();

    let all = table.list().await.unwrap();
    assert_eq!(all.len(), 1);
    assert!(!all.contains_key("a"));
}

#[tokio::test]
async fn test_delete_all() {
    let (_db, table) = setup().await;

    table.delete_all().await.unwrap();
    assert!(table.list().await.unwrap().is_empty());
}

// ── InsertableDataSet ──────────────────────────────────────────────────────

/// `insert_return_id` hands back the database's key, or the table's
/// generated one. A bare `TEXT PRIMARY KEY` with neither has no usable id:
/// SQL NULL comes back, and that is an error, not a "Null" string.
#[tokio::test]
async fn test_insert_return_id() {
    let (db, table) = setup().await;

    sqlx::query(
        "CREATE TABLE auto_item (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            price INTEGER NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();
    let auto = Table::<SqliteDB, Item>::new("auto_item", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");

    let item = Item {
        name: "Auto".into(),
        price: 42,
    };

    let db_id = auto.insert_return_id(&item).await.unwrap();
    assert!(!db_id.is_empty());
    assert_eq!(auto.get(db_id).await.unwrap(), Some(item.clone()));

    let err = table
        .insert_return_id(&item)
        .await
        .expect_err("a NULL primary key must not be returned as a usable id");
    assert!(
        !err.to_string().contains("Null"),
        "error should explain the NULL id, not echo it: {err}"
    );

    let minted = table.with_generated_id(IdGenerator::UuidV7);
    let minted_id = minted.insert_return_id(&item).await.unwrap();
    assert!(
        minted_id.contains('-'),
        "expected a uuid, got {minted_id:?}"
    );
    assert_eq!(minted.get(minted_id).await.unwrap(), Some(item));
}

#[tokio::test]
async fn test_generated_id_kept_on_explicit_insert() {
    // With a generator registered, the explicit-id insert path still writes the
    // id the caller passed — the generator never clobbers it.
    let (_db, table) = setup().await;
    let table = table.with_generated_id(IdGenerator::UuidV7);

    let item = Item {
        name: "Explicit".into(),
        price: 9,
    };
    assert_eq!(table.insert("z", &item).await.unwrap(), item);
    assert_eq!(table.get("z").await.unwrap(), Some(item));
}
