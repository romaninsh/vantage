//! The write contract on PostgreSQL: confined, idempotent, invariant-filling.

use vantage_sql::postgres::operation::PostgresOperation;
use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

/// A fresh `item_<test>` table, so tests running in parallel don't share rows.
async fn item(test: &str) -> Table<PostgresDB, EmptyEntity> {
    let db = PostgresDB::connect(PG_URL).await.unwrap();
    let name = format!("item_{test}");
    sqlx::query(&format!("DROP TABLE IF EXISTS \"{name}\""))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE \"{name}\" (id TEXT PRIMARY KEY, name TEXT, parent TEXT, price BIGINT)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    Table::new(&name, db)
        .with_id_column("id")
        .with_text_id()
        .with_column_of::<String>("name")
        .with_column_of::<String>("parent")
        .with_column_of::<i64>("price")
}

safe_writes_tests!(PostgresDB, AnyPostgresType);
