//! Test 2: ExprDataSource — execute Expression<AnyPostgresType> against live PostgreSQL.

use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
use vantage_types::Record;

const PG_URL: &str = "postgres://vantage:vantage@localhost:5432/vantage";

async fn setup(table: &str) -> PostgresDB {
    let db = PostgresDB::connect(PG_URL).await.unwrap();

    sqlx::query(&format!("DROP TABLE IF EXISTS `{}`", table))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE `{}` (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            price BIGINT NOT NULL,
            weight DOUBLE NOT NULL,
            active BOOLEAN NOT NULL
        )",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO `{}` VALUES (1, 'Apple', 100, 0.2, true), (2, 'Banana', 50, 0.15, true), (3, 'Cherry', 200, 0.01, false)",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    db
}

/// Helper: unwrap result into Vec of Record<AnyPostgresType>.
fn records(result: AnyPostgresType) -> Vec<Record<AnyPostgresType>> {
    Vec::<Record<AnyPostgresType>>::try_from(result).unwrap()
}

// ── Basic select via ExprDataSource ────────────────────────────────────────

#[tokio::test]
async fn test_select_all() {
    let db = setup("expr_select_all").await;
    let expr =
        Expression::<AnyPostgresType>::new("SELECT * FROM `expr_select_all` ORDER BY id", vec![]);
    let result = records(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 3);
    assert_eq!(
        result[0]["name"].try_get::<String>(),
        Some("Apple".to_string())
    );
    assert_eq!(
        result[2]["name"].try_get::<String>(),
        Some("Cherry".to_string())
    );
}

// ── Parameterized query ────────────────────────────────────────────────────

#[tokio::test]
async fn test_parameterized_integer() {
    let db = setup("expr_param_int").await;
    let expr = Expression::<AnyPostgresType>::new(
        "SELECT name FROM `expr_param_int` WHERE id = {}",
        vec![ExpressiveEnum::Scalar(AnyPostgresType::new(2i64))],
    );
    let result = records(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0]["name"].try_get::<String>(),
        Some("Banana".to_string())
    );
}

#[tokio::test]
async fn test_parameterized_text() {
    let db = setup("expr_param_text").await;
    let expr = Expression::<AnyPostgresType>::new(
        "SELECT price FROM `expr_param_text` WHERE name = {}",
        vec![ExpressiveEnum::Scalar(AnyPostgresType::new(
            "Cherry".to_string(),
        ))],
    );
    let result = records(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["price"].try_get::<i64>(), Some(200));
}

#[tokio::test]
async fn test_parameterized_bool() {
    let db = setup("expr_param_bool").await;
    let expr = Expression::<AnyPostgresType>::new(
        "SELECT name FROM `expr_param_bool` WHERE active = {} ORDER BY name",
        vec![ExpressiveEnum::Scalar(AnyPostgresType::new(true))],
    );
    let result = records(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 2);
    assert_eq!(
        result[0]["name"].try_get::<String>(),
        Some("Apple".to_string())
    );
    assert_eq!(
        result[1]["name"].try_get::<String>(),
        Some("Banana".to_string())
    );
}

// ── Multiple parameters ───────────────────────────────────────────────────

#[tokio::test]
async fn test_multiple_params() {
    let db = setup("expr_multi_params").await;
    let expr = Expression::<AnyPostgresType>::new(
        "SELECT name FROM `expr_multi_params` WHERE price >= {} AND active = {}",
        vec![
            ExpressiveEnum::Scalar(AnyPostgresType::new(100i64)),
            ExpressiveEnum::Scalar(AnyPostgresType::new(true)),
        ],
    );
    let result = records(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0]["name"].try_get::<String>(),
        Some("Apple".to_string())
    );
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
        "SELECT name FROM `expr_nested` WHERE {} ORDER BY name",
        vec![ExpressiveEnum::Nested(where_clause)],
    );

    let result = records(db.execute(&full_query).await.unwrap());
    assert_eq!(result.len(), 2);
    assert_eq!(
        result[0]["name"].try_get::<String>(),
        Some("Apple".to_string())
    );
    assert_eq!(
        result[1]["name"].try_get::<String>(),
        Some("Cherry".to_string())
    );
}

// ── Empty result ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_empty_result() {
    let db = setup("expr_empty").await;
    let expr = Expression::<AnyPostgresType>::new(
        "SELECT name FROM `expr_empty` WHERE id = {}",
        vec![ExpressiveEnum::Scalar(AnyPostgresType::new(999i64))],
    );
    let result = records(db.execute(&expr).await.unwrap());

    assert!(result.is_empty());
}
