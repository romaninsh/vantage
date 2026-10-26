//! Test 2c: SELECT into Record and deserialize into structs.
//!
//! Uses associate::<Record<JsonValue>> and TryFromRecord to verify
//! the full pipeline. Includes failure cases for field mismatches.
//!
//! All tests share pre-populated data via separate tables to avoid race conditions.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use vantage_expressions::ExprDataSource;
use vantage_sql::mysql::MysqlDB;
use vantage_sql::mysql_expr;
use vantage_types::{Record, TryFromRecord};

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

async fn setup(table: &str) -> MysqlDB {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();

    sqlx::query(&format!("DROP TABLE IF EXISTS `{}`", table))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE `{}` (
            id VARCHAR(255) PRIMARY KEY,
            name TEXT NOT NULL,
            price BIGINT NOT NULL,
            calories BIGINT NOT NULL,
            weight DOUBLE,
            is_deleted BOOLEAN NOT NULL DEFAULT false,
            description TEXT
        )",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO `{}` VALUES
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
async fn test_select_record_into_entity() {
    let db = setup("rec_entity").await;

    let cupcake: Record<JsonValue> = db
        .associate(mysql_expr!(
            "SELECT * FROM `rec_entity` WHERE id = {}",
            "cupcake"
        ))
        .get()
        .await
        .unwrap();
    let plain: Record<JsonValue> = db
        .associate(mysql_expr!(
            "SELECT * FROM `rec_entity` WHERE id = {}",
            "plain"
        ))
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
    let db = setup("rec_mismatch").await;

    let record: Record<JsonValue> = db
        .associate(mysql_expr!(
            "SELECT name, price FROM `rec_mismatch` WHERE id = {}",
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
        price: String, // BIGINT in DB
    }

    let missing = ProductWithRating::from_record(record.clone())
        .unwrap_err()
        .to_string();
    assert!(missing.contains("missing field `rating`"), "{missing}");
    let mistyped = WrongTypes::from_record(record).unwrap_err().to_string();
    assert!(mistyped.contains("invalid type"), "{mistyped}");
}
