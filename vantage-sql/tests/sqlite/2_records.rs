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
        "cupcake",
        "Flux Cupcake",
        120i64,
        300i64,
        0.25f64,
        false,
        "A tasty cupcake",
        "tart",
        "Time Tart",
        220i64,
        200i64,
        0.15f64,
        true,
        "tart"
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
async fn test_select_record_into_entity() {
    let db = setup().await;

    let cupcake: Record<JsonValue> = db
        .associate(sqlite_expr!(
            "SELECT * FROM product WHERE id = {}",
            "cupcake"
        ))
        .get()
        .await
        .unwrap();
    let plain: Record<JsonValue> = db
        .associate(sqlite_expr!("SELECT * FROM product WHERE id = {}", "plain"))
        .get()
        .await
        .unwrap();

    assert_eq!(
        Product::from_record(cupcake).unwrap(),
        Product {
            id: "cupcake".into(),
            name: "Flux Cupcake".into(),
            price: 120,
            calories: 300,
            weight: Some(0.25),
            is_deleted: false,
            description: Some("A tasty cupcake".into()),
        }
    );
    // NULL columns land as None
    assert_eq!(
        Product::from_record(plain).unwrap(),
        Product {
            id: "plain".into(),
            name: "Plain Bread".into(),
            price: 50,
            calories: 150,
            weight: None,
            is_deleted: false,
            description: None,
        }
    );
}

#[tokio::test]
async fn test_missing_or_mistyped_field_fails() {
    let db = setup().await;

    let record: Record<JsonValue> = db
        .associate(sqlite_expr!(
            "SELECT name, price FROM product WHERE id = {}",
            "cupcake"
        ))
        .get()
        .await
        .unwrap();

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct ProductWithRating {
        name: String,
        rating: f64, // not selected
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct WrongTypes {
        name: i64,     // TEXT in DB
        price: String, // INTEGER in DB
    }

    let missing = ProductWithRating::from_record(record.clone())
        .unwrap_err()
        .to_string();
    assert!(missing.contains("missing field `rating`"), "{missing}");
    let mistyped = WrongTypes::from_record(record).unwrap_err().to_string();
    assert!(mistyped.contains("invalid type"), "{mistyped}");
}
