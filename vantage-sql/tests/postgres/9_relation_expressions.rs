//! Expression ids, relation expressions and `sql::concat` on PostgreSQL: see
//! `common/expression_ids.rs`, `common/relation_aggregates.rs` and
//! `common/rhai_concat.rs`.

use vantage_expressions::Expression;
#[allow(unused_imports)]
use vantage_sql::postgres::operation::PostgresOperation;
use vantage_sql::postgres::{AnyPostgresType, PostgresDB, pg_ident};
use vantage_sql::postgres_expr;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

async fn connect() -> PostgresDB {
    PostgresDB::connect("postgres://vantage:vantage@localhost:5433/vantage")
        .await
        .unwrap()
}

fn quote(name: &str) -> String {
    format!("\"{name}\"")
}

const TEXT: &str = "TEXT";

fn insert_ignore(table: &str, values: &str) -> String {
    format!("INSERT INTO {table} VALUES {values} ON CONFLICT DO NOTHING")
}

fn link_id() -> Expression<AnyPostgresType> {
    postgres_expr!(
        "{}::text || ':' || {}::text",
        (pg_ident("a")),
        (pg_ident("b"))
    )
}

expression_ids_tests!(PostgresDB, AnyPostgresType);
relation_aggregates_tests!(PostgresDB, AnyPostgresType);
rhai_concat_tests!(
    PostgresDB,
    AnyPostgresType,
    vantage_sql::postgres::statements::PostgresSelect,
    vantage_sql::postgres::statements::select::join::PostgresSelectJoin,
    vantage_sql::condition::PostgresCondition,
    r#""a" || {} || "b""#
);

/// Inside a transaction a duplicate link is found before the INSERT, so the
/// statement never fails and the transaction stays usable.
#[tokio::test]
async fn expression_id_relink_in_a_transaction_is_existed() {
    use vantage_dataset::Inserted;
    use vantage_sql::postgres::operation::PostgresOperation as _;
    use vantage_vista::Transaction;
    let all = expression_ids::link("expression_id_relink_in_a_transaction_is_existed").await;
    let a = all["a"].clone();
    let rec = |b: i64| -> vantage_types::Record<AnyPostgresType> {
        [("b".to_string(), AnyPostgresType::from(b))]
            .into_iter()
            .collect()
    };
    all.clone()
        .with_condition(a.eq(7i64))
        .insert_derived_value(&rec(5))
        .await
        .unwrap();

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
