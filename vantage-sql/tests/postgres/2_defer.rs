//! Test 2b: Deferred expressions — cross-database value resolution.
//!
//! A deferred expression runs a query on one database at execution time,
//! and the resulting value gets placed as a parameter in another database's query.

use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
use vantage_sql::postgres_expr;
use vantage_types::Record;

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

fn records(result: AnyPostgresType) -> Vec<Record<AnyPostgresType>> {
    Vec::<Record<AnyPostgresType>>::try_from(result).unwrap()
}

async fn setup(prefix: &str) -> (PostgresDB, PostgresDB) {
    // Both use the same postgres instance but different tables
    let db = PostgresDB::connect(PG_URL).await.unwrap();

    let config_table = format!("{}_config", prefix);
    let product_table = format!("{}_product", prefix);

    sqlx::query(&format!("DROP TABLE IF EXISTS \"{}\"", config_table))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(&format!("DROP TABLE IF EXISTS \"{}\"", product_table))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE \"{}\" (key TEXT PRIMARY KEY, value BIGINT NOT NULL)",
        config_table
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO \"{}\" VALUES ('min_price', 150)",
        config_table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE \"{}\" (id TEXT PRIMARY KEY, name TEXT NOT NULL, price BIGINT NOT NULL)",
        product_table
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO \"{}\" VALUES ('a', 'Cheap Thing', 50), ('b', 'Mid Thing', 150), ('c', 'Expensive Thing', 300)",
        product_table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    // Return same pool as two "databases" (using different tables)
    (db.clone(), db)
}

// ── defer() resolves a value from one query, binds it into another ────────

#[tokio::test]
async fn test_deferred_parameter() {
    let (config_db, shop_db) = setup("defer").await;

    let threshold_query = postgres_expr!(
        "SELECT value FROM \"defer_config\" WHERE key = {}",
        "min_price"
    );
    let deferred_threshold = config_db.defer(threshold_query);

    let inner = Expression::<AnyPostgresType>::new(
        "price >= {}",
        vec![ExpressiveEnum::Deferred(deferred_threshold)],
    );
    let shop_query = Expression::<AnyPostgresType>::new(
        "SELECT name FROM \"defer_product\" WHERE {} AND name != {} ORDER BY price",
        vec![
            ExpressiveEnum::Nested(inner),
            ExpressiveEnum::Scalar(AnyPostgresType::new("Expensive Thing".to_string())),
        ],
    );

    let result = records(shop_db.execute(&shop_query).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0]["name"].try_get::<String>(),
        Some("Mid Thing".to_string())
    );
}

// ── Deferred query that finds nothing ──────────────────────────────────────

#[tokio::test]
async fn test_deferred_without_a_row_resolves_to_no_rows() {
    let (config_db, _) = setup("defer4").await;

    let missing = postgres_expr!(
        "SELECT value FROM \"defer4_config\" WHERE key = {}",
        "no_such_key"
    );
    let resolved = match config_db.defer(missing).call().await.unwrap() {
        ExpressiveEnum::Scalar(value) => value,
        _ => panic!("a deferred query resolves to a scalar"),
    };

    assert!(records(resolved).is_empty());
}
