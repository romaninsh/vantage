//! Relation expressions on PostgreSQL: see `common/relation_aggregates.rs`.

use vantage_expressions::Expression;
use vantage_sql::postgres::{AnyPostgresType, PostgresDB, pg_ident};
use vantage_sql::postgres_expr;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

/// The tables are shared by every test and filled once; a statement that
/// loses a race with another test's setup is retried.
async fn setup() -> PostgresDB {
    let db = PostgresDB::connect("postgres://vantage:vantage@localhost:5433/vantage")
        .await
        .unwrap();
    for sql in [
        "CREATE TABLE IF NOT EXISTS rx_project (id INTEGER PRIMARY KEY, name TEXT)",
        "CREATE TABLE IF NOT EXISTS rx_tag (id INTEGER PRIMARY KEY, name TEXT)",
        "CREATE TABLE IF NOT EXISTS rx_project_tag (a INTEGER NOT NULL, b INTEGER NOT NULL, PRIMARY KEY (a, b))",
        "CREATE TABLE IF NOT EXISTS rx_task (id INTEGER PRIMARY KEY, project_id INTEGER, parent_id INTEGER, active INTEGER)",
        "CREATE TABLE IF NOT EXISTS rx_update (id INTEGER PRIMARY KEY, project_id INTEGER, date TEXT, status TEXT)",
        "INSERT INTO rx_project VALUES (7, 'Seven'), (8, 'Eight') ON CONFLICT DO NOTHING",
        "INSERT INTO rx_tag VALUES (3, 'alpha'), (5, 'beta') ON CONFLICT DO NOTHING",
        "INSERT INTO rx_project_tag VALUES (7, 5), (7, 3) ON CONFLICT DO NOTHING",
        "INSERT INTO rx_task VALUES (1, 7, NULL, 1), (2, 7, 1, 1), (3, 7, 1, 0), (4, 8, NULL, 1) ON CONFLICT DO NOTHING",
        "INSERT INTO rx_update VALUES (1, 7, '2026-01-01', 'off_track'), (2, 7, '2026-02-01', 'on_track') ON CONFLICT DO NOTHING",
    ] {
        if sqlx::query(sql).execute(db.pool()).await.is_err() {
            sqlx::query(sql).execute(db.pool()).await.unwrap();
        }
    }
    db
}

fn link_id() -> Expression<AnyPostgresType> {
    postgres_expr!(
        "{}::text || ':' || {}::text",
        (pg_ident("a")),
        (pg_ident("b"))
    )
}

relation_aggregates_tests!(PostgresDB, AnyPostgresType);
