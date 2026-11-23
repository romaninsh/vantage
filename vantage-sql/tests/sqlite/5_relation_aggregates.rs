//! Relation expressions on SQLite: see `common/relation_aggregates.rs`.

use vantage_expressions::Expression;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB, sqlite_ident};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    for sql in [
        "CREATE TABLE IF NOT EXISTS rx_project (id INTEGER PRIMARY KEY, name TEXT)",
        "CREATE TABLE IF NOT EXISTS rx_tag (id INTEGER PRIMARY KEY, name TEXT)",
        "CREATE TABLE IF NOT EXISTS rx_project_tag (a INTEGER NOT NULL, b INTEGER NOT NULL, PRIMARY KEY (a, b))",
        "CREATE TABLE IF NOT EXISTS rx_task (id INTEGER PRIMARY KEY, project_id INTEGER, parent_id INTEGER, active INTEGER)",
        "CREATE TABLE IF NOT EXISTS rx_update (id INTEGER PRIMARY KEY, project_id INTEGER, date TEXT, status TEXT)",
        "INSERT INTO rx_project VALUES (7, 'Seven'), (8, 'Eight')",
        "INSERT INTO rx_tag VALUES (3, 'alpha'), (5, 'beta')",
        "INSERT INTO rx_project_tag VALUES (7, 5), (7, 3)",
        "INSERT INTO rx_task VALUES (1, 7, NULL, 1), (2, 7, 1, 1), (3, 7, 1, 0), (4, 8, NULL, 1)",
        "INSERT INTO rx_update VALUES (1, 7, '2026-01-01', 'off_track'), (2, 7, '2026-02-01', 'on_track')",
    ] {
        sqlx::query(sql).execute(db.pool()).await.unwrap();
    }
    db
}

fn link_id() -> Expression<AnySqliteType> {
    sqlite_expr!("{} || ':' || {}", (sqlite_ident("a")), (sqlite_ident("b")))
}

relation_aggregates_tests!(SqliteDB, AnySqliteType);
