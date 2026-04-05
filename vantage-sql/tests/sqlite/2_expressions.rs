//! Test 2: ExprDataSource — execute Expression<AnySqliteType> against live SQLite.

use serde_json::Value as JsonValue;
use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    sqlx::query(
        "CREATE TABLE items (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            price INTEGER NOT NULL,
            weight REAL NOT NULL,
            active BOOLEAN NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query("INSERT INTO items VALUES (1, 'Apple', 100, 0.2, 1)")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO items VALUES (2, 'Banana', 50, 0.15, 1)")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO items VALUES (3, 'Cherry', 200, 0.01, 0)")
        .execute(db.pool())
        .await
        .unwrap();

    db
}

/// Helper: unwrap result into JSON array of row objects.
fn rows(result: AnySqliteType) -> Vec<JsonValue> {
    match result.into_value() {
        JsonValue::Array(arr) => arr,
        other => panic!("expected array, got: {:?}", other),
    }
}

// ── Basic select via ExprDataSource ────────────────────────────────────────

#[tokio::test]
async fn test_select_all() {
    let db = setup().await;
    let expr = Expression::<AnySqliteType>::new("SELECT * FROM items ORDER BY id", vec![]);
    let result = rows(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 3);
    assert_eq!(result[0]["name"], "Apple");
    assert_eq!(result[2]["name"], "Cherry");
}

// ── Parameterized query ────────────────────────────────────────────────────

#[tokio::test]
async fn test_parameterized_integer() {
    let db = setup().await;
    let expr = Expression::<AnySqliteType>::new(
        "SELECT name FROM items WHERE id = {}",
        vec![ExpressiveEnum::Scalar(AnySqliteType::new(2i64))],
    );
    let result = rows(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["name"], "Banana");
}

#[tokio::test]
async fn test_parameterized_text() {
    let db = setup().await;
    let expr = Expression::<AnySqliteType>::new(
        "SELECT price FROM items WHERE name = {}",
        vec![ExpressiveEnum::Scalar(AnySqliteType::new(
            "Cherry".to_string(),
        ))],
    );
    let result = rows(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["price"], 200);
}

#[tokio::test]
async fn test_parameterized_bool() {
    let db = setup().await;
    let expr = Expression::<AnySqliteType>::new(
        "SELECT name FROM items WHERE active = {} ORDER BY name",
        vec![ExpressiveEnum::Scalar(AnySqliteType::new(true))],
    );
    let result = rows(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 2);
    assert_eq!(result[0]["name"], "Apple");
    assert_eq!(result[1]["name"], "Banana");
}

// ── Multiple parameters ───────────────────────────────────────────────────

#[tokio::test]
async fn test_multiple_params() {
    let db = setup().await;
    let expr = Expression::<AnySqliteType>::new(
        "SELECT name FROM items WHERE price >= {} AND active = {}",
        vec![
            ExpressiveEnum::Scalar(AnySqliteType::new(100i64)),
            ExpressiveEnum::Scalar(AnySqliteType::new(true)),
        ],
    );
    let result = rows(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["name"], "Apple");
}

// ── Nested expressions ────────────────────────────────────────────────────

#[tokio::test]
async fn test_nested_expression() {
    let db = setup().await;

    let where_clause = Expression::<AnySqliteType>::new(
        "price > {}",
        vec![ExpressiveEnum::Scalar(AnySqliteType::new(75i64))],
    );
    let full_query = Expression::<AnySqliteType>::new(
        "SELECT name FROM items WHERE {} ORDER BY name",
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
    let db = setup().await;
    let expr = Expression::<AnySqliteType>::new(
        "SELECT name FROM items WHERE id = {}",
        vec![ExpressiveEnum::Scalar(AnySqliteType::new(999i64))],
    );
    let result = rows(db.execute(&expr).await.unwrap());

    assert!(result.is_empty());
}
