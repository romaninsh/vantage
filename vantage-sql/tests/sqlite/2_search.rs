//! Test 2f: search_table_expr LIKE escaping — verifies that %, _, and \ in
//! search values don't act as wildcards.

use vantage_dataset::ReadableDataSet;
#[allow(unused_imports)]
use vantage_sql::sqlite::{AnySqliteType, SqliteDB, SqliteType};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::entity;

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Item {
    name: String,
}

async fn setup(rows: &[&str]) -> (SqliteDB, Table<SqliteDB, Item>) {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    sqlx::query("CREATE TABLE item (id TEXT PRIMARY KEY, name TEXT NOT NULL)")
        .execute(db.pool())
        .await
        .unwrap();

    for (i, name) in rows.iter().enumerate() {
        sqlx::query("INSERT INTO item (id, name) VALUES (?, ?)")
            .bind(i.to_string())
            .bind(*name)
            .execute(db.pool())
            .await
            .unwrap();
    }

    let table = Table::<SqliteDB, Item>::new("item", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name");
    (db, table)
}

#[tokio::test]
async fn test_search_percent_literal() {
    let (db, table) = setup(&["100% organic", "regular item", "50% off"]).await;
    let condition = db.search_table_expr(&table, "100%");
    let mut table = table;
    table.add_condition(condition);
    let results = table.list().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results.values().next().unwrap().name, "100% organic");
}

#[tokio::test]
async fn test_search_underscore_literal() {
    let (db, table) = setup(&["a_b", "axb", "a__b", "axxb"]).await;
    let condition = db.search_table_expr(&table, "a_b");
    let mut table = table;
    table.add_condition(condition);
    let results = table.list().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results.values().next().unwrap().name, "a_b");
}

#[tokio::test]
async fn test_search_backslash_literal() {
    let (db, table) = setup(&["path\\to\\file", "pathXtoXfile", "other"]).await;
    let condition = db.search_table_expr(&table, "\\to\\");
    let mut table = table;
    table.add_condition(condition);
    let results = table.list().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results.values().next().unwrap().name, "path\\to\\file");
}
