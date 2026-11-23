//! Expression ids on SQLite: see `common/expression_ids.rs`.

use vantage_sql::sqlite::operation::SqliteOperation;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB, sqlite_ident};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

/// A fresh in-memory database per test, so `_test` needs no table of its own.
async fn link(_test: &str) -> Table<SqliteDB, EmptyEntity> {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    let name = "link".to_string();
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS \"{name}\"")))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE TABLE \"{name}\" (a INTEGER NOT NULL, b INTEGER, stage INTEGER, UNIQUE (a, b))"
    )))
    .execute(db.pool())
    .await
    .unwrap();
    Table::new(&name, db)
        .with_id_column("id")
        .with_expression("id", |_| {
            sqlite_expr!("{} || ':' || {}", (sqlite_ident("a")), (sqlite_ident("b")))
        })
        .with_column_of::<i64>("a")
        .with_column_of::<i64>("b")
        .with_column_of::<i64>("stage")
}

/// [`link`] with `b NOT NULL`.
async fn strict_link(_test: &str) -> Table<SqliteDB, EmptyEntity> {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    let name = "link".to_string();
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS \"{name}\"")))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE TABLE \"{name}\" (a INTEGER NOT NULL, b INTEGER NOT NULL, stage INTEGER, UNIQUE (a, b))"
    )))
    .execute(db.pool())
    .await
    .unwrap();
    Table::new(&name, db)
        .with_id_column("id")
        .with_expression("id", |_| {
            sqlite_expr!("{} || ':' || {}", (sqlite_ident("a")), (sqlite_ident("b")))
        })
        .with_column_of::<i64>("a")
        .with_column_of::<i64>("b")
        .with_column_of::<i64>("stage")
}

expression_ids_tests!(SqliteDB, AnySqliteType);
