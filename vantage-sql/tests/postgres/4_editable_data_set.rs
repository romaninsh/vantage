//! Test 4: WritableDataSet and InsertableDataSet for Table<PostgresDB, Entity>.

#[allow(unused_imports)]
use vantage_sql::postgres::PostgresType;
use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::{InsertableDataSet, ReadableDataSet, WritableDataSet};

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

#[entity(PostgresType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Item {
    name: String,
    price: i64,
}

async fn setup(suffix: &str) -> (PostgresDB, Table<PostgresDB, Item>) {
    let db = PostgresDB::connect(PG_URL).await.unwrap();
    let table_name = format!("edit_item_{}", suffix);

    sqlx::query(&format!("DROP TABLE IF EXISTS \"{}\"", table_name))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE \"{}\" (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            price BIGINT NOT NULL
        )",
        table_name
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO \"{}\" VALUES ('a', 'Alpha', 10), ('b', 'Beta', 20)",
        table_name
    ))
    .execute(db.pool())
    .await
    .unwrap();

    let table = Table::<PostgresDB, Item>::new(&table_name, db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");
    (db, table)
}

#[tokio::test]
async fn test_insert() {
    let (_db, table) = setup("insert").await;

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
    let (_db, table) = setup("replace").await;

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
    let (_db, table) = setup("patch").await;

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
    let (_db, table) = setup("delete").await;

    table.delete(&"a".to_string()).await.unwrap();

    let all = table.list().await.unwrap();
    assert_eq!(all.len(), 1);
    assert!(!all.contains_key("a"));
}

#[tokio::test]
async fn test_delete_all() {
    let (_db, table) = setup("delete_all").await;

    table.delete_all().await.unwrap();
    assert!(table.list().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_insert_return_id() {
    let (db, _) = setup("auto_id").await;

    sqlx::query("DROP TABLE IF EXISTS \"edit_auto_item\"")
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(
        "CREATE TABLE \"edit_auto_item\" (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            price BIGINT NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    let table = Table::<PostgresDB, Item>::new("edit_auto_item", db)
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
