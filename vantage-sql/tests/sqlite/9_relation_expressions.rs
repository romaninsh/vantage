//! Expression ids and relation expressions on SQLite: see
//! `common/expression_ids.rs` and `common/relation_aggregates.rs`.

use vantage_expressions::Expression;
#[allow(unused_imports)]
use vantage_sql::sqlite::operation::SqliteOperation;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB, sqlite_ident};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

async fn connect() -> SqliteDB {
    SqliteDB::connect("sqlite::memory:").await.unwrap()
}

fn quote(name: &str) -> String {
    format!("\"{name}\"")
}

const TEXT: &str = "TEXT";

fn insert_ignore(table: &str, values: &str) -> String {
    format!("INSERT OR IGNORE INTO {table} VALUES {values}")
}

fn link_id() -> Expression<AnySqliteType> {
    sqlite_expr!("{} || ':' || {}", (sqlite_ident("a")), (sqlite_ident("b")))
}

expression_ids_tests!(SqliteDB, AnySqliteType);
relation_aggregates_tests!(SqliteDB, AnySqliteType);
