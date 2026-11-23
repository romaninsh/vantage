//! Relation expressions on MySQL: see `common/relation_aggregates.rs`.

use vantage_expressions::Expression;
use vantage_sql::mysql::{AnyMysqlType, MysqlDB, mysql_ident};
use vantage_sql::mysql_expr;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

/// The tables are shared by every test and filled once; a statement that
/// loses a race with another test's setup is retried.
async fn setup() -> MysqlDB {
    let db = MysqlDB::connect("mysql://vantage:vantage@localhost:3306/vantage")
        .await
        .unwrap();
    for sql in [
        "CREATE TABLE IF NOT EXISTS rx_project (id INTEGER PRIMARY KEY, name VARCHAR(64))",
        "CREATE TABLE IF NOT EXISTS rx_tag (id INTEGER PRIMARY KEY, name VARCHAR(64))",
        "CREATE TABLE IF NOT EXISTS rx_project_tag (a INTEGER NOT NULL, b INTEGER NOT NULL, PRIMARY KEY (a, b))",
        "CREATE TABLE IF NOT EXISTS rx_task (id INTEGER PRIMARY KEY, project_id INTEGER, parent_id INTEGER, active INTEGER)",
        "CREATE TABLE IF NOT EXISTS rx_update (id INTEGER PRIMARY KEY, project_id INTEGER, date VARCHAR(64), status VARCHAR(64))",
        "INSERT IGNORE INTO rx_project VALUES (7, 'Seven'), (8, 'Eight')",
        "INSERT IGNORE INTO rx_tag VALUES (3, 'alpha'), (5, 'beta')",
        "INSERT IGNORE INTO rx_project_tag VALUES (7, 5), (7, 3)",
        "INSERT IGNORE INTO rx_task VALUES (1, 7, NULL, 1), (2, 7, 1, 1), (3, 7, 1, 0), (4, 8, NULL, 1)",
        "INSERT IGNORE INTO rx_update VALUES (1, 7, '2026-01-01', 'off_track'), (2, 7, '2026-02-01', 'on_track')",
    ] {
        if sqlx::query(sql).execute(db.pool()).await.is_err() {
            sqlx::query(sql).execute(db.pool()).await.unwrap();
        }
    }
    db
}

fn link_id() -> Expression<AnyMysqlType> {
    mysql_expr!(
        "CONCAT({}, ':', {})",
        (mysql_ident("a")),
        (mysql_ident("b"))
    )
}

relation_aggregates_tests!(MysqlDB, AnyMysqlType);
