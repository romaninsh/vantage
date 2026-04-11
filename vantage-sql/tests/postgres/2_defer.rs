//! Test 2b: Deferred expressions — cross-database value resolution.
//!
//! A deferred expression runs a query on one database at execution time,
//! and the resulting value gets placed as a parameter in another database's query.

use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
use vantage_sql::postgres_expr;
use vantage_types::Record;

const PG_URL: &str = "postgres://vantage:vantage@localhost:5432/vantage";

fn records(result: AnyPostgresType) -> Vec<Record<AnyPostgresType>> {
    Vec::<Record<AnyPostgresType>>::try_from(result).unwrap()
}

async fn setup(prefix: &str) -> (PostgresDB, PostgresDB) {
    // Both use the same postgres instance but different tables
    let db = PostgresDB::connect(PG_URL).await.unwrap();

    let config_table = format!("{}_config", prefix);
    let product_table = format!("{}_product", prefix);

    sqlx::query(&format!("DROP TABLE IF EXISTS `{}`", config_table))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(&format!("DROP TABLE IF EXISTS `{}`", product_table))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE `{}` (`key` VARCHAR(255) PRIMARY KEY, value BIGINT NOT NULL)",
        config_table
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO `{}` VALUES ('min_price', 150)",
        config_table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE `{}` (id VARCHAR(255) PRIMARY KEY, name TEXT NOT NULL, price BIGINT NOT NULL)",
        product_table
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO `{}` VALUES ('a', 'Cheap Thing', 50), ('b', 'Mid Thing', 150), ('c', 'Expensive Thing', 300)",
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
async fn test_cross_database_deferred() {
    let (config_db, shop_db) = setup("defer1").await;

    let threshold_query = postgres_expr!(
        "SELECT value FROM `defer1_config` WHERE `key` = {}",
        "min_price"
    );
    let deferred_threshold = config_db.defer(threshold_query);

    let shop_query = Expression::<AnyPostgresType>::new(
        "SELECT name FROM `defer1_product` WHERE price >= {} ORDER BY price",
        vec![ExpressiveEnum::Deferred(deferred_threshold)],
    );

    let result = records(shop_db.execute(&shop_query).await.unwrap());

    assert_eq!(result.len(), 2);
    assert_eq!(
        result[0]["name"].try_get::<String>(),
        Some("Mid Thing".to_string())
    );
    assert_eq!(
        result[1]["name"].try_get::<String>(),
        Some("Expensive Thing".to_string())
    );
}

// ── Deferred mixed with regular scalar parameters ──────────────────────────

#[tokio::test]
async fn test_deferred_mixed_with_scalars() {
    let (config_db, shop_db) = setup("defer2").await;

    let threshold_query = postgres_expr!(
        "SELECT value FROM `defer2_config` WHERE `key` = {}",
        "min_price"
    );
    let deferred_threshold = config_db.defer(threshold_query);

    let shop_query = Expression::<AnyPostgresType>::new(
        "SELECT name FROM `defer2_product` WHERE price >= {} AND name != {} ORDER BY price",
        vec![
            ExpressiveEnum::Deferred(deferred_threshold),
            ExpressiveEnum::Scalar(AnyPostgresType::new("Mid Thing".to_string())),
        ],
    );

    let result = records(shop_db.execute(&shop_query).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0]["name"].try_get::<String>(),
        Some("Expensive Thing".to_string())
    );
}

// ── Deferred inside a nested expression ───────────────────────────────────

#[tokio::test]
async fn test_nested_deferred() {
    let (config_db, shop_db) = setup("defer3").await;

    let threshold_query = postgres_expr!(
        "SELECT value FROM `defer3_config` WHERE `key` = {}",
        "min_price"
    );
    let deferred_threshold = config_db.defer(threshold_query);

    let inner = Expression::<AnyPostgresType>::new(
        "price >= {}",
        vec![ExpressiveEnum::Deferred(deferred_threshold)],
    );

    let shop_query = postgres_expr!(
        "SELECT name FROM `defer3_product` WHERE {} ORDER BY price",
        (inner)
    );

    let result = records(shop_db.execute(&shop_query).await.unwrap());

    assert_eq!(result.len(), 2);
    assert_eq!(
        result[0]["name"].try_get::<String>(),
        Some("Mid Thing".to_string())
    );
    assert_eq!(
        result[1]["name"].try_get::<String>(),
        Some("Expensive Thing".to_string())
    );
}
