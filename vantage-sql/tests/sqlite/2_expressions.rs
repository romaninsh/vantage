//! Test 2: ExprDataSource — execute Expression<AnySqliteType> against live SQLite.

use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_types::Record;

const SQLITE_URL: &str = "sqlite::memory:";

async fn setup(table: &str) -> SqliteDB {
    let db = SqliteDB::connect(SQLITE_URL).await.unwrap();

    sqlx::query(&format!(
        "CREATE TABLE \"{}\" (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            price INTEGER NOT NULL,
            weight REAL NOT NULL,
            active BOOLEAN NOT NULL
        )",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO \"{}\" VALUES (1, 'Apple', 100, 0.2, 1), (2, 'Banana', 50, 0.15, 1), (3, 'Cherry', 200, 0.01, 0)",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    db
}

/// Helper: unwrap result into Vec of Record<AnySqliteType>.
fn records(result: AnySqliteType) -> Vec<Record<AnySqliteType>> {
    Vec::<Record<AnySqliteType>>::try_from(result).unwrap()
}

// ── Scalar parameters, top level and nested ───────────────────────────────

#[tokio::test]
async fn test_scalar_parameters() {
    let db = setup("expr_params").await;

    let price_above = Expression::<AnySqliteType>::new(
        "price > {}",
        vec![ExpressiveEnum::Scalar(AnySqliteType::new(75i64))],
    );
    // (price > 75 AND active = true) OR name = 'Cherry'
    let expr = Expression::<AnySqliteType>::new(
        "SELECT name FROM \"expr_params\" WHERE {} AND active = {} OR name = {} ORDER BY name",
        vec![
            ExpressiveEnum::Nested(price_above),
            ExpressiveEnum::Scalar(AnySqliteType::new(true)),
            ExpressiveEnum::Scalar(AnySqliteType::new("Cherry".to_string())),
        ],
    );
    let result = records(db.execute(&expr).await.unwrap());

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
    let expr = Expression::<AnySqliteType>::new(
        "SELECT name FROM \"expr_empty\" WHERE id = {}",
        vec![ExpressiveEnum::Scalar(AnySqliteType::new(999i64))],
    );
    let result = records(db.execute(&expr).await.unwrap());

    assert!(result.is_empty());
}
