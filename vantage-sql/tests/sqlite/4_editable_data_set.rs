//! Test 4: WritableDataSet and InsertableDataSet for Table<SqliteDB, Entity>.
//!
//! Uses in-memory SQLite with a typed entity.

#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
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
async fn test_insert() {
    let (_db, table) = setup().await;

    let item = Item {
        name: "Gamma".into(),
        price: 30,
    };
    let result = table.insert(&"c".to_string(), &item).await.unwrap();
    assert_eq!(result.name, "Gamma");
    assert_eq!(result.price, 30);

    let fetched = table.get("c").await.unwrap();
    assert_eq!(fetched.name, "Gamma");
}

#[tokio::test]
async fn test_replace() {
    let (_db, table) = setup().await;

    let item = Item {
        name: "Alpha Replaced".into(),
        price: 99,
    };
    table.replace(&"a".to_string(), &item).await.unwrap();

    let fetched = table.get("a").await.unwrap();
    assert_eq!(fetched.name, "Alpha Replaced");
    assert_eq!(fetched.price, 99);
}

#[tokio::test]
async fn test_patch() {
    let (_db, table) = setup().await;

    let partial = Item {
        name: "".into(),
        price: 55,
    };
    table.patch(&"a".to_string(), &partial).await.unwrap();

    let fetched = table.get("a").await.unwrap();
    assert_eq!(fetched.price, 55);
}

#[tokio::test]
async fn test_delete() {
    let (_db, table) = setup().await;

    table.delete(&"a".to_string()).await.unwrap();

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

#[tokio::test]
async fn test_insert_return_id() {
    let (db, _) = setup().await;

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

    let table = Table::<SqliteDB, Item>::new("auto_item", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");

    let item = Item {
        name: "Auto".into(),
        price: 42,
    };
    let id = table.insert_return_id(&item).await.unwrap();
    assert!(!id.is_empty());

    let fetched = table.get(id).await.unwrap();
    assert_eq!(fetched.name, "Auto");
    assert_eq!(fetched.price, 42);
}
