//! Test 2: ExprDataSource — execute Expression<AnyPostgresType> against live PostgreSQL.

use serde_json::Value as JsonValue;
use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::postgres::{AnyPostgresType, PostgresDB};

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

async fn setup(table: &str) -> PostgresDB {
    let db = PostgresDB::connect(PG_URL).await.unwrap();

    sqlx::query(&format!("DROP TABLE IF EXISTS \"{}\"", table))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE \"{}\" (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            price BIGINT NOT NULL,
            weight DOUBLE PRECISION NOT NULL,
            active BOOLEAN NOT NULL
        )",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO \"{}\" VALUES (1, 'Apple', 100, 0.2, true), (2, 'Banana', 50, 0.15, true), (3, 'Cherry', 200, 0.01, false)",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    db
}

/// Helper: unwrap result into JSON array of row objects.
fn rows(result: AnyPostgresType) -> Vec<JsonValue> {
    match result.into_value() {
        JsonValue::Array(arr) => arr,
        other => panic!("expected array, got: {:?}", other),
    }
}

// ── Basic select via ExprDataSource ────────────────────────────────────────

#[tokio::test]
async fn test_select_all() {
    let db = setup("expr_select_all").await;
    let expr =
        Expression::<AnyPostgresType>::new("SELECT * FROM \"expr_select_all\" ORDER BY id", vec![]);
    let result = rows(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 3);
    assert_eq!(result[0]["name"], "Apple");
    assert_eq!(result[2]["name"], "Cherry");
}

// ── Parameterized query ────────────────────────────────────────────────────

#[tokio::test]
async fn test_parameterized_integer() {
    let db = setup("expr_param_int").await;
    let expr = Expression::<AnyPostgresType>::new(
        "SELECT name FROM \"expr_param_int\" WHERE id = {}",
        vec![ExpressiveEnum::Scalar(AnyPostgresType::new(2i64))],
    );
    let result = rows(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["name"], "Banana");
}

#[tokio::test]
async fn test_parameterized_text() {
    let db = setup("expr_param_text").await;
    let expr = Expression::<AnyPostgresType>::new(
        "SELECT price FROM \"expr_param_text\" WHERE name = {}",
        vec![ExpressiveEnum::Scalar(AnyPostgresType::new(
            "Cherry".to_string(),
        ))],
    );
    let result = rows(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["price"], 200);
}

#[tokio::test]
async fn test_parameterized_bool() {
    let db = setup("expr_param_bool").await;
    let expr = Expression::<AnyPostgresType>::new(
        "SELECT name FROM \"expr_param_bool\" WHERE active = {} ORDER BY name",
        vec![ExpressiveEnum::Scalar(AnyPostgresType::new(true))],
    );
    let result = rows(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 2);
    assert_eq!(result[0]["name"], "Apple");
    assert_eq!(result[1]["name"], "Banana");
}

// ── Multiple parameters ───────────────────────────────────────────────────

#[tokio::test]
async fn test_multiple_params() {
    let db = setup("expr_multi_params").await;
    let expr = Expression::<AnyPostgresType>::new(
        "SELECT name FROM \"expr_multi_params\" WHERE price >= {} AND active = {}",
        vec![
            ExpressiveEnum::Scalar(AnyPostgresType::new(100i64)),
            ExpressiveEnum::Scalar(AnyPostgresType::new(true)),
        ],
    );
    let result = rows(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["name"], "Apple");
}

// ── Nested expressions ────────────────────────────────────────────────────

#[tokio::test]
async fn test_nested_expression() {
    let db = setup("expr_nested").await;

    let where_clause = Expression::<AnyPostgresType>::new(
        "price > {}",
        vec![ExpressiveEnum::Scalar(AnyPostgresType::new(75i64))],
    );
    let full_query = Expression::<AnyPostgresType>::new(
        "SELECT name FROM \"expr_nested\" WHERE {} ORDER BY name",
        vec![ExpressiveEnum::Nested(where_clause)],
    );

    let result = rows(db.execute(&full_query).await.unwrap());
    assert_eq!(result.len(), 2);
    assert_eq!(result[0]["name"], "Apple");
    assert_eq!(result[1]["name"], "Cherry");
}

// ── Empty result ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_empty_result() {
    let db = setup("expr_empty").await;
    let expr = Expression::<AnyPostgresType>::new(
        "SELECT name FROM \"expr_empty\" WHERE id = {}",
        vec![ExpressiveEnum::Scalar(AnyPostgresType::new(999i64))],
    );
    let result = rows(db.execute(&expr).await.unwrap());

    assert!(result.is_empty());
}
