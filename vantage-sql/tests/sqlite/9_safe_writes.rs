//! The write contract on SQLite: confined, idempotent, invariant-filling.

use vantage_sql::sqlite::operation::SqliteOperation;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

/// A fresh in-memory database per test, so `_test` needs no table of its own.
async fn item(_test: &str) -> Table<SqliteDB, EmptyEntity> {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE item (id TEXT PRIMARY KEY, name TEXT, parent TEXT, price INTEGER)")
        .execute(db.pool())
        .await
        .unwrap();
    Table::new("item", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("parent")
        .with_column_of::<i64>("price")
}

safe_writes_tests!(SqliteDB, AnySqliteType);
