//! Test 2f: search_table_condition LIKE escaping — verifies that %, _, and \ in
//! search values don't act as wildcards.

use vantage_dataset::ReadableDataSet;
#[allow(unused_imports)]
use vantage_sql::mysql::{AnyMysqlType, MysqlDB, MysqlType};
use vantage_sql::mysql_expr;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::entity;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

#[entity(MysqlType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Item {
    name: String,
}

#[entity(MysqlType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Doc {
    name: String,
    status: String,
}

async fn setup(suffix: &str, rows: &[&str]) -> (MysqlDB, Table<MysqlDB, Item>) {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
    let table_name = format!("search_{}", suffix);

    sqlx::query(&format!("DROP TABLE IF EXISTS `{}`", table_name))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE `{}` (id VARCHAR(255) PRIMARY KEY, name TEXT NOT NULL)",
        table_name
    ))
    .execute(db.pool())
    .await
    .unwrap();

    for (i, name) in rows.iter().enumerate() {
        sqlx::query(&format!(
            "INSERT INTO `{}` (id, name) VALUES (?, ?)",
            table_name
        ))
        .bind(i.to_string())
        .bind(*name)
        .execute(db.pool())
        .await
        .unwrap();
    }

    let table = Table::<MysqlDB, Item>::new(&table_name, db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name");
    (db, table)
}

#[tokio::test]
async fn test_search_percent_literal() {
    let (db, table) = setup("pct", &["100% organic", "regular item", "50% off"]).await;
    let condition = db.search_table_condition(&table, "100%");
    let mut table = table;
    table.add_condition(condition);
    let results = table.list().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results.values().next().unwrap().name, "100% organic");
}

#[tokio::test]
async fn test_search_underscore_literal() {
    let (db, table) = setup("usc", &["a_b", "axb", "a__b", "axxb"]).await;
    let condition = db.search_table_condition(&table, "a_b");
    let mut table = table;
    table.add_condition(condition);
    let results = table.list().await.unwrap();
    // Only "a_b" should match, not "axb"
    assert_eq!(results.len(), 1);
    assert_eq!(results.values().next().unwrap().name, "a_b");
}

#[tokio::test]
async fn test_search_backslash_literal() {
    let (db, table) = setup("bs", &["path\\to\\file", "pathXtoXfile", "other"]).await;
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
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
    sqlx::query("DROP TABLE IF EXISTS `search_cond`")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE `search_cond` (id VARCHAR(255) PRIMARY KEY, name TEXT NOT NULL, status TEXT NOT NULL)",
    )
    .execute(db.pool())
    .await
    .unwrap();
    for (id, name, status) in [("1", "alpha", "registered"), ("2", "beta", "draft")] {
        sqlx::query("INSERT INTO `search_cond` (id, name, status) VALUES (?, ?, ?)")
            .bind(id)
            .bind(name)
            .bind(status)
            .execute(db.pool())
            .await
            .unwrap();
    }

    let table = Table::<MysqlDB, Doc>::new("search_cond", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("status")
        .with_condition(mysql_expr!("`status` = {}", "registered"));

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
