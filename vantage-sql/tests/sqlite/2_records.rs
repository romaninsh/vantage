//! Test 2c: SELECT into Record and deserialize into structs.
//!
//! Uses associate::<Record<JsonValue>> and TryFromRecord to verify
//! the full pipeline. Includes failure cases for field mismatches.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use vantage_expressions::ExprDataSource;
use vantage_sql::sqlite::SqliteDB;
use vantage_sql::sqlite_expr;
use vantage_types::{Record, TryFromRecord};

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    sqlx::query(
        "CREATE TABLE product (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            price INTEGER NOT NULL,
            calories INTEGER NOT NULL,
            weight REAL,
            is_deleted BOOLEAN NOT NULL DEFAULT 0,
            description TEXT
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    let insert = sqlite_expr!(
        "INSERT INTO product VALUES ({}, {}, {}, {}, {}, {}, {}), ({}, {}, {}, {}, {}, {}, {})",
        "cupcake", "Flux Cupcake", 120i64, 300i64, 0.25f64, false, "A tasty cupcake",
        "tart", "Time Tart", 220i64, 200i64, 0.15f64, true, "tart"
    );
    db.execute(&insert).await.unwrap();

    sqlx::query("INSERT INTO product (id, name, price, calories, is_deleted) VALUES ('plain', 'Plain Bread', 50, 150, 0)")
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
    let db = setup().await;

    let record: Record<JsonValue> = db
        .associate(sqlite_expr!("SELECT * FROM product WHERE id = {}", "cupcake"))
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
    let db = setup().await;

    let record: Record<JsonValue> = db
        .associate(sqlite_expr!("SELECT name, price, weight, description FROM product WHERE id = {}", "plain"))
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
    let db = setup().await;

    let record: Record<JsonValue> = db
        .associate(sqlite_expr!("SELECT * FROM product WHERE id = {}", "cupcake"))
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
    let db = setup().await;

    let record: Record<JsonValue> = db
        .associate(sqlite_expr!("SELECT name, weight FROM product WHERE id = {}", "plain"))
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
    let db = setup().await;

    let record: Record<JsonValue> = db
        .associate(sqlite_expr!("SELECT name, price FROM product WHERE id = {}", "cupcake"))
        .get()
        .await
        .unwrap();

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct WrongTypes {
        name: i64,     // TEXT in DB
        price: String, // INTEGER in DB
    }

    assert!(WrongTypes::from_record(record).is_err());
}
