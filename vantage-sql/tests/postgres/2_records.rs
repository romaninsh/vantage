//! Test 2c: SELECT into Record and deserialize into structs.
//!
//! Uses associate::<Record<JsonValue>> and TryFromRecord to verify
//! the full pipeline. Includes failure cases for field mismatches.
//!
//! All tests share pre-populated data via separate tables to avoid race conditions.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use vantage_expressions::ExprDataSource;
use vantage_sql::postgres::PostgresDB;
use vantage_sql::postgres_expr;
use vantage_types::{Record, TryFromRecord};

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

async fn setup(table: &str) -> PostgresDB {
    let db = PostgresDB::connect(PG_URL).await.unwrap();

    sqlx::query(&format!("DROP TABLE IF EXISTS \"{}\"", table))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE \"{}\" (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            price BIGINT NOT NULL,
            calories BIGINT NOT NULL,
            weight DOUBLE PRECISION,
            is_deleted BOOLEAN NOT NULL DEFAULT false,
            description TEXT
        )",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO \"{}\" VALUES
            ('cupcake', 'Flux Cupcake', 120, 300, 0.25, false, 'A tasty cupcake'),
            ('tart', 'Time Tart', 220, 200, 0.15, true, 'tart'),
            ('plain', 'Plain Bread', 50, 150, NULL, false, NULL)",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    db
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
struct Product {
    id: String,
    name: String,
    price: i64,
    calories: i64,
    weight: Option<f64>,
    is_deleted: bool,
    description: Option<String>,
}

#[tokio::test]
async fn test_select_single_record_into_entity() {
    let db = setup("rec_entity").await;

    let record: Record<JsonValue> = db
        .associate(postgres_expr!(
            "SELECT * FROM \"rec_entity\" WHERE id = {}",
            "cupcake"
        ))
        .get()
        .await
        .unwrap();

    let product: Product = Product::from_record(record).unwrap();
    assert_eq!(product.name, "Flux Cupcake");
    assert_eq!(product.price, 120);
    assert!((product.weight.unwrap() - 0.25).abs() < f64::EPSILON);
    assert_eq!(product.description, Some("A tasty cupcake".to_string()));
    assert!(!product.is_deleted);
}

#[tokio::test]
async fn test_null_into_optional_field() {
    let db = setup("rec_optional").await;

    let record: Record<JsonValue> = db
        .associate(postgres_expr!(
            "SELECT name, price, weight, description FROM \"rec_optional\" WHERE id = {}",
            "plain"
        ))
        .get()
        .await
        .unwrap();

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct ProductOptional {
        name: String,
        price: i64,
        weight: Option<f64>,
        description: Option<String>,
    }

    let product: ProductOptional = ProductOptional::from_record(record).unwrap();
    assert_eq!(product.name, "Plain Bread");
    assert_eq!(product.weight, None);
    assert_eq!(product.description, None);
}

#[tokio::test]
async fn test_missing_field_fails() {
    let db = setup("rec_missing").await;

    let record: Record<JsonValue> = db
        .associate(postgres_expr!(
            "SELECT * FROM \"rec_missing\" WHERE id = {}",
            "cupcake"
        ))
        .get()
        .await
        .unwrap();

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct ProductWithRating {
        name: String,
        rating: f64, // doesn't exist
    }

    assert!(ProductWithRating::from_record(record).is_err());
}

#[tokio::test]
async fn test_null_into_required_field_fails() {
    let db = setup("rec_null_required").await;

    let record: Record<JsonValue> = db
        .associate(postgres_expr!(
            "SELECT name, weight FROM \"rec_null_required\" WHERE id = {}",
            "plain"
        ))
        .get()
        .await
        .unwrap();

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Strict {
        name: String,
        weight: f64, // NOT Option — plain has NULL here
    }

    assert!(Strict::from_record(record).is_err());
}

#[tokio::test]
async fn test_wrong_field_type_fails() {
    let db = setup("rec_wrong_type").await;

    let record: Record<JsonValue> = db
        .associate(postgres_expr!(
            "SELECT name, price FROM \"rec_wrong_type\" WHERE id = {}",
            "cupcake"
        ))
        .get()
        .await
        .unwrap();

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct WrongTypes {
        name: i64,     // TEXT in DB
        price: String, // BIGINT in DB
    }

    assert!(WrongTypes::from_record(record).is_err());
}
