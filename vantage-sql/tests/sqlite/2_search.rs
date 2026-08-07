//! Test 2f: search_table_condition LIKE escaping — verifies that %, _, and \ in
//! search values don't act as wildcards.

use vantage_dataset::ReadableDataSet;
#[allow(unused_imports)]
use vantage_sql::sqlite::{AnySqliteType, SqliteDB, SqliteType};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::entity;

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Item {
    name: String,
}

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Doc {
    name: String,
    status: String,
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
    let condition = db.search_table_condition(&table, "100%");
    let mut table = table;
    table.add_condition(condition);
    let results = table.list().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results.values().next().unwrap().name, "100% organic");
}

#[tokio::test]
async fn test_search_underscore_literal() {
    let (db, table) = setup(&["a_b", "axb", "a__b", "axxb"]).await;
    let condition = db.search_table_condition(&table, "a_b");
    let mut table = table;
    table.add_condition(condition);
    let results = table.list().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results.values().next().unwrap().name, "a_b");
}

#[tokio::test]
async fn test_search_backslash_literal() {
    let (db, table) = setup(&["path\\to\\file", "pathXtoXfile", "other"]).await;
    let condition = db.search_table_condition(&table, "\\to\\");
    let mut table = table;
    table.add_condition(condition);
    let results = table.list().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results.values().next().unwrap().name, "path\\to\\file");
}

/// A search must keep the conditions of the table. The search looks in
/// each column and joins the branches with `OR`, and `AND` binds more
/// tightly than `OR`. Without a group, the query reads as
/// `(status = 'registered' AND <branch 1>) OR <branch 2> …`, and each
/// row that matches a later branch ignores the status condition.
#[tokio::test]
async fn test_search_keeps_table_conditions() {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE doc (id TEXT PRIMARY KEY, name TEXT NOT NULL, status TEXT NOT NULL)")
        .execute(db.pool())
        .await
        .unwrap();
    for (id, name, status) in [("1", "alpha", "registered"), ("2", "beta", "draft")] {
        sqlx::query("INSERT INTO doc (id, name, status) VALUES (?, ?, ?)")
            .bind(id)
            .bind(name)
            .bind(status)
            .execute(db.pool())
            .await
            .unwrap();
    }

    let table = Table::<SqliteDB, Doc>::new("doc", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("status")
        .with_condition(sqlite_expr!("\"status\" = {}", "registered"));

    // "draft" matches the status column of row 2 only. Row 2 fails the
    // status condition, so the search finds no rows.
    let mut filtered = table.clone();
    filtered.add_condition(db.search_table_condition(&table, "draft"));
    assert_eq!(filtered.list().await.unwrap().len(), 0);

    // A search that matches a row which meets the condition finds it.
    let mut filtered = table.clone();
    filtered.add_condition(db.search_table_condition(&table, "alpha"));
    let results = filtered.list().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results.values().next().unwrap().name, "alpha");
}
