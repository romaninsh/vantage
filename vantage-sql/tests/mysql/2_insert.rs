//! Test 2a: INSERT via Expression<AnyMysqlType> + ExprDataSource.
//!
//! No statement builders — raw expressions with typed parameters.
//! Focuses on the Product table to exercise multiple types (text, integer, bool).

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
#[allow(unused_imports)]
use vantage_expressions::Expressive;
use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::mysql::{AnyMysqlType, MysqlDB};
use vantage_sql::mysql_expr;
use vantage_types::{Record, TryFromRecord};

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
struct Product {
    name: String,
    price: i64,
    calories: i64,
    is_deleted: bool,
    inventory_stock: i64,
}

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
            is_deleted BOOLEAN NOT NULL DEFAULT false,
            inventory_stock BIGINT NOT NULL DEFAULT 0
        )",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    db
}

/// Helper: execute expression, unwrap result into JSON rows.
fn rows(result: AnyMysqlType) -> Vec<JsonValue> {
    match result.into_value() {
        JsonValue::Array(arr) => arr,
        other => panic!("expected array, got: {:?}", other),
    }
}

// ── Insert with typed parameters ───────────────────────────────────────────

#[tokio::test]
async fn test_insert_product() {
    let db = setup("ins_product").await;

    let insert = mysql_expr!(
        "INSERT INTO `ins_product` (`id`, `name`, `price`, `calories`, `is_deleted`, `inventory_stock`) VALUES ({}, {}, {}, {}, {}, {})",
        "cupcake",
        "Flux Cupcake",
        120i64,
        300i64,
        false,
        50i64
    );

    db.execute(&insert).await.unwrap();

    let select = mysql_expr!(
        "SELECT name, price, calories, is_deleted, inventory_stock FROM `ins_product` WHERE id = {}",
        "cupcake"
    );
    let result = rows(db.execute(&select).await.unwrap());

    assert_eq!(result.len(), 1);

    let record: Record<JsonValue> = result[0].clone().into();
    let product: Product = Product::from_record(record).unwrap();

    assert_eq!(product.name, "Flux Cupcake");
    assert_eq!(product.price, 120);
    assert_eq!(product.calories, 300);
    assert!(!product.is_deleted);
    assert_eq!(product.inventory_stock, 50);
}

// ── Insert multiple rows via nested expressions ───────────────────────────

#[tokio::test]
async fn test_insert_multiple_products() {
    let db = setup("ins_multi").await;

    let row1 = mysql_expr!(
        "({}, {}, {}, {}, {}, {})",
        "tart",
        "Time Tart",
        220i64,
        200i64,
        false,
        20i64
    );
    let row2 = mysql_expr!(
        "({}, {}, {}, {}, {}, {})",
        "donut",
        "DeLorean Doughnut",
        135i64,
        250i64,
        false,
        30i64
    );
    let row3 = mysql_expr!(
        "({}, {}, {}, {}, {}, {})",
        "pie",
        "Sea Pie",
        299i64,
        350i64,
        true,
        0i64
    );

    let rows_expr = Expression::from_vec(vec![row1, row2, row3], ", ");
    let insert = Expression::<AnyMysqlType>::new(
        "INSERT INTO `ins_multi` (`id`, `name`, `price`, `calories`, `is_deleted`, `inventory_stock`) VALUES {}",
        vec![ExpressiveEnum::Nested(rows_expr)],
    );

    db.execute(&insert).await.unwrap();

    let select = mysql_expr!(
        "SELECT name, price, calories, is_deleted, inventory_stock FROM `ins_multi` ORDER BY price"
    );
    let result = rows(db.execute(&select).await.unwrap());

    assert_eq!(result.len(), 3);

    let parsed: Vec<Product> = result
        .into_iter()
        .map(|r| Product::from_record(r.into()).unwrap())
        .collect();

    assert_eq!(parsed[0].name, "DeLorean Doughnut");
    assert_eq!(parsed[0].price, 135);
    assert!(!parsed[0].is_deleted);

    assert_eq!(parsed[2].name, "Sea Pie");
    assert_eq!(parsed[2].price, 299);
    assert!(parsed[2].is_deleted);
    assert_eq!(parsed[2].inventory_stock, 0);
}

// ── Type marker verification ────────────────────────────────────────────────

#[tokio::test]
async fn test_bool_binds_correctly() {
    let db = setup("ins_bool").await;

    let insert = mysql_expr!(
        "INSERT INTO `ins_bool` (`id`, `name`, `price`, `calories`, `is_deleted`, `inventory_stock`) VALUES ({}, {}, {}, {}, {}, {})",
        "deleted_item",
        "Gone",
        100i64,
        100i64,
        true,
        0i64
    );
    db.execute(&insert).await.unwrap();

    // Query with bool parameter — MySQL BOOLEAN is TINYINT(1)
    let select = mysql_expr!("SELECT name FROM `ins_bool` WHERE is_deleted = {}", true);
    let result = rows(db.execute(&select).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["name"], "Gone");
}

#[tokio::test]
async fn test_integer_vs_text_binding() {
    let db = setup("ins_int_text").await;

    let insert = mysql_expr!(
        "INSERT INTO `ins_int_text` (`id`, `name`, `price`, `calories`, `is_deleted`, `inventory_stock`) VALUES ({}, {}, {}, {}, {}, {})",
        "item1",
        "Test",
        100i64,
        100i64,
        false,
        10i64
    );
    db.execute(&insert).await.unwrap();

    let by_price = mysql_expr!("SELECT id FROM `ins_int_text` WHERE price = {}", 100i64);
    let result = rows(db.execute(&by_price).await.unwrap());
    assert_eq!(result.len(), 1);

    let by_name = mysql_expr!("SELECT id FROM `ins_int_text` WHERE name = {}", "Test");
    let result = rows(db.execute(&by_name).await.unwrap());
    assert_eq!(result.len(), 1);
}
