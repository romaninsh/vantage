//! Expression ids on PostgreSQL: see `common/expression_ids.rs`.

use vantage_sql::postgres::operation::PostgresOperation;
use vantage_sql::postgres::{AnyPostgresType, PostgresDB, pg_ident};
use vantage_sql::postgres_expr;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

const URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

/// A fresh `link_<test>` table, so tests running in parallel don't share rows.
async fn link(test: &str) -> Table<PostgresDB, EmptyEntity> {
    let db = PostgresDB::connect(URL).await.unwrap();
    let name = format!("link_{test}");
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
            postgres_expr!("{}::text || ':' || {}::text", (pg_ident("a")), (pg_ident("b")))
        })
        .with_column_of::<i64>("a")
        .with_column_of::<i64>("b")
        .with_column_of::<i64>("stage")
}

/// [`link`] with `b NOT NULL`.
async fn strict_link(test: &str) -> Table<PostgresDB, EmptyEntity> {
    let db = PostgresDB::connect(URL).await.unwrap();
    let name = format!("strict_{test}");
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
            postgres_expr!("{}::text || ':' || {}::text", (pg_ident("a")), (pg_ident("b")))
        })
        .with_column_of::<i64>("a")
        .with_column_of::<i64>("b")
        .with_column_of::<i64>("stage")
}

expression_ids_tests!(PostgresDB, AnyPostgresType);

/// Inside a transaction a duplicate link is found before the INSERT, so the
/// statement never fails and the transaction stays usable.
#[tokio::test]
async fn expression_id_relink_in_a_transaction_is_existed() {
    use vantage_dataset::Inserted;
    use vantage_sql::postgres::operation::PostgresOperation as _;
    use vantage_vista::Transaction;
    let all = link("expression_id_relink_in_a_transaction_is_existed").await;
    let a = all["a"].clone();
    let rec = |b: i64| -> vantage_types::Record<AnyPostgresType> {
        [("b".to_string(), AnyPostgresType::from(b))].into_iter().collect()
    };
    all.clone().with_condition(a.eq(7i64)).insert_derived_value(&rec(5)).await.unwrap();

    let tx = Transaction::new();
    let db = all.data_source().in_transaction(&tx).unwrap();
    let set = all.clone().with_data_source(db).with_condition(a.eq(7i64));
    let (_, again) = set.insert_derived_value(&rec(5)).await.unwrap();
    assert!(matches!(again, Inserted::Existed(_)), "{again:?}");
    let (id, new) = set.insert_derived_value(&rec(6)).await.unwrap();
    assert_eq!(id, "7:6");
    assert!(matches!(new, Inserted::Created(_)), "{new:?}");
    tx.commit().await.unwrap();
    assert_eq!(all.get_count().await.unwrap(), 2);
}
