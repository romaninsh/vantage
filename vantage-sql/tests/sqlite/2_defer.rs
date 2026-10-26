//! Test 2b: Deferred expressions — cross-database value resolution.
//!
//! A deferred expression runs a query on one database at execution time,
//! and the resulting value gets placed as a parameter in another database's query.

use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;
use vantage_types::Record;

fn records(result: AnySqliteType) -> Vec<Record<AnySqliteType>> {
    Vec::<Record<AnySqliteType>>::try_from(result).unwrap()
}

async fn setup() -> (SqliteDB, SqliteDB) {
    let config_db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    let shop_db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    // Config database: stores thresholds
    sqlx::query("CREATE TABLE config (key TEXT PRIMARY KEY, value INTEGER NOT NULL)")
        .execute(config_db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO config VALUES ('min_price', 150)")
        .execute(config_db.pool())
        .await
        .unwrap();

    // Shop database: stores products
    sqlx::query(
        "CREATE TABLE product (id TEXT PRIMARY KEY, name TEXT NOT NULL, price INTEGER NOT NULL)",
    )
    .execute(shop_db.pool())
    .await
    .unwrap();

    let insert = sqlite_expr!(
        "INSERT INTO product VALUES ({}, {}, {}), ({}, {}, {}), ({}, {}, {})",
        "a",
        "Cheap Thing",
        50i64,
        "b",
        "Mid Thing",
        150i64,
        "c",
        "Expensive Thing",
        300i64
    );
    shop_db.execute(&insert).await.unwrap();

    (config_db, shop_db)
}

// ── defer() resolves a value from one DB, binds it into another ────────────

#[tokio::test]
async fn test_deferred_parameter() {
    let (config_db, shop_db) = setup().await;

    // At execution, shop_db runs the deferred query on config_db (150) first
    let threshold_query = sqlite_expr!("SELECT value FROM config WHERE key = {}", "min_price");
    let deferred_threshold = config_db.defer(threshold_query);

    let inner = Expression::<AnySqliteType>::new(
        "price >= {}",
        vec![ExpressiveEnum::Deferred(deferred_threshold)],
    );
    let shop_query = Expression::<AnySqliteType>::new(
        "SELECT name FROM product WHERE {} AND name != {} ORDER BY price",
        vec![
            ExpressiveEnum::Nested(inner),
            ExpressiveEnum::Scalar(AnySqliteType::new("Expensive Thing".to_string())),
        ],
    );

    let result = records(shop_db.execute(&shop_query).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0]["name"].try_get::<String>(),
        Some("Mid Thing".to_string())
    );
}
