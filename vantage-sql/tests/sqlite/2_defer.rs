//! Test 2b: Deferred expressions — cross-database value resolution.
//!
//! A deferred expression runs a query on one database at execution time,
//! and the resulting value gets placed as a parameter in another database's
//! query. This is NOT a subquery — the deferred query executes first,
//! produces a concrete value, and that value gets bound into the outer query.

use serde_json::Value as JsonValue;
use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;

fn rows(result: AnySqliteType) -> Vec<JsonValue> {
    match result.into_value() {
        JsonValue::Array(arr) => arr,
        other => panic!("expected array, got: {:?}", other),
    }
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
async fn test_cross_database_deferred() {
    let (config_db, shop_db) = setup().await;

    // defer() on config_db: at execution time, runs the query and extracts
    // the scalar value (150). That value gets bound into the shop_db query.
    let threshold_query = sqlite_expr!("SELECT value FROM config WHERE key = {}", "min_price");
    let deferred_threshold = config_db.defer(threshold_query);

    let shop_query = Expression::<AnySqliteType>::new(
        "SELECT name FROM product WHERE price >= {} ORDER BY price",
        vec![ExpressiveEnum::Deferred(deferred_threshold)],
    );

    // shop_db.execute() resolves the deferred first (calls config_db),
    // gets 150, then runs: SELECT name FROM product WHERE price >= 150
    let result = rows(shop_db.execute(&shop_query).await.unwrap());

    assert_eq!(result.len(), 2);
    assert_eq!(result[0]["name"], "Mid Thing");
    assert_eq!(result[1]["name"], "Expensive Thing");
}

// ── Deferred mixed with regular scalar parameters ──────────────────────────

#[tokio::test]
async fn test_deferred_mixed_with_scalars() {
    let (config_db, shop_db) = setup().await;

    let threshold_query = sqlite_expr!("SELECT value FROM config WHERE key = {}", "min_price");
    let deferred_threshold = config_db.defer(threshold_query);

    let shop_query = Expression::<AnySqliteType>::new(
        "SELECT name FROM product WHERE price >= {} AND name != {} ORDER BY price",
        vec![
            ExpressiveEnum::Deferred(deferred_threshold),
            ExpressiveEnum::Scalar(AnySqliteType::new("Mid Thing".to_string())),
        ],
    );

    let result = rows(shop_db.execute(&shop_query).await.unwrap());

    // 150 threshold, excluding "Mid Thing" → only "Expensive Thing"
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["name"], "Expensive Thing");
}

// ── Deferred inside a nested expression ───────────────────────────────────

#[tokio::test]
async fn test_nested_deferred() {
    let (config_db, shop_db) = setup().await;

    let threshold_query = sqlite_expr!("SELECT value FROM config WHERE key = {}", "min_price");
    let deferred_threshold = config_db.defer(threshold_query);

    // Build a nested expression: the deferred lives inside an inner expression
    // that gets composed into the outer query via sqlite_expr!(... (inner) ...)
    let inner = Expression::<AnySqliteType>::new(
        "price >= {}",
        vec![ExpressiveEnum::Deferred(deferred_threshold)],
    );

    let shop_query = sqlite_expr!("SELECT name FROM product WHERE {} ORDER BY price", (inner));

    let result = rows(shop_db.execute(&shop_query).await.unwrap());

    assert_eq!(result.len(), 2);
    assert_eq!(result[0]["name"], "Mid Thing");
    assert_eq!(result[1]["name"], "Expensive Thing");
}
