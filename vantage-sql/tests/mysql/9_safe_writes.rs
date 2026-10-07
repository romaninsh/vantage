//! The write contract on MySQL: confined, idempotent, invariant-filling.

use vantage_sql::mysql::operation::MysqlOperation;
use vantage_sql::mysql::{AnyMysqlType, MysqlDB};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

/// A fresh `item_<test>` table, so tests running in parallel don't share rows.
async fn item(test: &str) -> Table<MysqlDB, EmptyEntity> {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
    let name = format!("item_{test}");
    sqlx::query(&format!("DROP TABLE IF EXISTS `{name}`"))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE `{name}` (id VARCHAR(64) PRIMARY KEY, name TEXT, parent TEXT, price BIGINT)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    Table::new(&name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("parent")
        .with_column_of::<i64>("price")
}

safe_writes_tests!(MysqlDB, AnyMysqlType);
