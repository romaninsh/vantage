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
async fn test_insert() {
    let (_db, table) = setup().await;

    let item = Item {
        name: "Gamma".into(),
        price: 30,
    };
    let result = table.insert("c", &item).await.unwrap();
    assert_eq!(result.name, "Gamma");
    assert_eq!(result.price, 30);

    let fetched = table.get("c").await.unwrap().expect("row c");
    assert_eq!(fetched.name, "Gamma");
}

#[tokio::test]
async fn test_replace() {
    let (_db, table) = setup().await;

    let item = Item {
        name: "Alpha Replaced".into(),
        price: 99,
    };
    table.replace("a", &item).await.unwrap();

    let fetched = table.get("a").await.unwrap().expect("row a");
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
    table.patch("a", &partial).await.unwrap();

    let fetched = table.get("a").await.unwrap().expect("row a");
    assert_eq!(fetched.price, 55);
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

    let fetched = table.get(id).await.unwrap().expect("inserted row");
    assert_eq!(fetched.name, "Auto");
    assert_eq!(fetched.price, 42);
}

#[tokio::test]
async fn test_insert_return_id_null_pk_errors() {
    // `item`'s id is a bare `TEXT PRIMARY KEY` — no DEFAULT, no AUTOINCREMENT.
    // Omitting the id inserts SQL NULL, so RETURNING hands back a NULL id, which
    // is not usable. It must surface as an error, not a bogus "Null" string.
    let (_db, table) = setup().await;

    let item = Item {
        name: "Orphan".into(),
        price: 7,
    };
    let err = table
        .insert_return_id(&item)
        .await
        .expect_err("a NULL primary key must not be returned as a usable id");
    let msg = err.to_string();
    assert!(
        !msg.contains("Null"),
        "error should explain the NULL id, not echo it: {msg}"
    );
}

#[tokio::test]
async fn test_generated_id_fills_bare_primary_key() {
    // Same bare `TEXT PRIMARY KEY` as above, but `with_generated_id` mints the id
    // client-side before the INSERT, so `insert_return_id` succeeds and hands
    // back the generated key.
    let (_db, table) = setup().await;
    let table = table.with_generated_id(IdGenerator::UuidV7);

    let item = Item {
        name: "Minted".into(),
        price: 7,
    };
    let id = table.insert_return_id(&item).await.unwrap();
    assert!(id.contains('-'), "expected a uuid, got {id:?}");

    let fetched = table.get(id).await.unwrap().expect("inserted row");
    assert_eq!(fetched.name, "Minted");
    assert_eq!(fetched.price, 7);
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
    table.insert("z", &item).await.unwrap();
    let fetched = table.get("z").await.unwrap().expect("row z");
    assert_eq!(fetched.name, "Explicit");
}
