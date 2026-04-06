//! Test 2f: search_table_expr LIKE escaping — verifies that %, _, and \ in
//! search values don't act as wildcards.

use vantage_dataset::ReadableDataSet;
#[allow(unused_imports)]
use vantage_sql::postgres::{AnyPostgresType, PostgresDB, PostgresType};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::entity;

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

#[entity(PostgresType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Item {
    name: String,
}

async fn setup(suffix: &str, rows: &[&str]) -> (PostgresDB, Table<PostgresDB, Item>) {
    let db = PostgresDB::connect(PG_URL).await.unwrap();
    let table_name = format!("search_{}", suffix);

    sqlx::query(&format!("DROP TABLE IF EXISTS \"{}\"", table_name))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE \"{}\" (id TEXT PRIMARY KEY, name TEXT NOT NULL)",
        table_name
    ))
    .execute(db.pool())
    .await
    .unwrap();

    for (i, name) in rows.iter().enumerate() {
        sqlx::query(&format!(
            "INSERT INTO \"{}\" (id, name) VALUES ($1, $2)",
            table_name
        ))
        .bind(i.to_string())
        .bind(*name)
        .execute(db.pool())
        .await
        .unwrap();
    }

    let table = Table::<PostgresDB, Item>::new(&table_name, db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name");
    (db, table)
}

#[tokio::test]
async fn test_search_percent_literal() {
    let (db, table) = setup("pct", &["100% organic", "regular item", "50% off"]).await;
    let condition = db.search_table_expr(&table, "100%");
    let mut table = table;
    table.add_condition(condition);
    let results = table.list().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results.values().next().unwrap().name, "100% organic");
}

#[tokio::test]
async fn test_search_underscore_literal() {
    let (db, table) = setup("usc", &["a_b", "axb", "a__b", "axxb"]).await;
    let condition = db.search_table_expr(&table, "a_b");
    let mut table = table;
    table.add_condition(condition);
    let results = table.list().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results.values().next().unwrap().name, "a_b");
}

#[tokio::test]
async fn test_search_backslash_literal() {
    let (db, table) = setup("bs", &["path\\to\\file", "pathXtoXfile", "other"]).await;
    let condition = db.search_table_expr(&table, "\\to\\");
    let mut table = table;
    table.add_condition(condition);
    let results = table.list().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results.values().next().unwrap().name, "path\\to\\file");
}
