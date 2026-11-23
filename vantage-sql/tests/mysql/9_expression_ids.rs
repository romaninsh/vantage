//! Expression ids on MySQL: see `common/expression_ids.rs`.

use vantage_sql::mysql::operation::MysqlOperation;
use vantage_sql::mysql::{AnyMysqlType, MysqlDB, mysql_ident};
use vantage_sql::mysql_expr;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

const URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

/// A fresh `link_<test>` table, so tests running in parallel don't share rows.
async fn link(test: &str) -> Table<MysqlDB, EmptyEntity> {
    let db = MysqlDB::connect(URL).await.unwrap();
    let name = format!("link_{test}");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP TABLE IF EXISTS `{name}`"
    )))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE TABLE `{name}` (a INTEGER NOT NULL, b INTEGER, stage INTEGER, UNIQUE (a, b))"
    )))
    .execute(db.pool())
    .await
    .unwrap();
    Table::new(&name, db)
        .with_id_column("id")
        .with_expression("id", |_| {
            mysql_expr!(
                "CONCAT({}, ':', {})",
                (mysql_ident("a")),
                (mysql_ident("b"))
            )
        })
        .with_column_of::<i64>("a")
        .with_column_of::<i64>("b")
        .with_column_of::<i64>("stage")
}

/// [`link`] with `b NOT NULL`.
async fn strict_link(test: &str) -> Table<MysqlDB, EmptyEntity> {
    let db = MysqlDB::connect(URL).await.unwrap();
    let name = format!("strict_{test}");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP TABLE IF EXISTS `{name}`"
    )))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE TABLE `{name}` (a INTEGER NOT NULL, b INTEGER NOT NULL, stage INTEGER, UNIQUE (a, b))"
    )))
    .execute(db.pool())
    .await
    .unwrap();
    Table::new(&name, db)
        .with_id_column("id")
        .with_expression("id", |_| {
            mysql_expr!(
                "CONCAT({}, ':', {})",
                (mysql_ident("a")),
                (mysql_ident("b"))
            )
        })
        .with_column_of::<i64>("a")
        .with_column_of::<i64>("b")
        .with_column_of::<i64>("stage")
}

expression_ids_tests!(MysqlDB, AnyMysqlType);
