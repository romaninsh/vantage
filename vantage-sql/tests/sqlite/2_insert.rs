//! Test 2a: INSERT via Expression<AnySqliteType> + ExprDataSource.
//!
//! No statement builders — raw expressions with typed parameters.
//! Focuses on the Product table to exercise multiple types (text, integer, bool).

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
#[allow(unused_imports)]
use vantage_expressions::Expressive;
use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;
use vantage_types::{Record, TryFromRecord};

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
struct Product {
    name: String,
    price: i64,
    calories: i64,
    is_deleted: bool,
    inventory_stock: i64,
}

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    sqlx::query(
        "CREATE TABLE product (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            price INTEGER NOT NULL,
            calories INTEGER NOT NULL,
            is_deleted BOOLEAN NOT NULL DEFAULT 0,
            inventory_stock INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    db
}

/// Helper: execute expression, unwrap result into JSON rows.
fn rows(result: AnySqliteType) -> Vec<JsonValue> {
    match result.into_value() {
        JsonValue::Array(arr) => arr,
        other => panic!("expected array, got: {:?}", other),
    }
}

// ── Insert with typed parameters ───────────────────────────────────────────

#[tokio::test]
async fn test_insert_product() {
    let db = setup().await;

    // INSERT using sqlite_expr! — each parameter is AnySqliteType with proper variant
    let insert = sqlite_expr!(
        "INSERT INTO \"product\" (\"id\", \"name\", \"price\", \"calories\", \"is_deleted\", \"inventory_stock\") VALUES ({}, {}, {}, {}, {}, {})",
        "cupcake",      // Text
        "Flux Cupcake", // Text
        120i64,         // Integer
        300i64,         // Integer
        false,          // Integer (bool → 0)
        50i64           // Integer
    );

    db.execute(&insert).await.unwrap();

    // Read back and verify
    let select = sqlite_expr!(
        "SELECT name, price, calories, is_deleted, inventory_stock FROM product WHERE id = {}",
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

// ── Insert multiple rows via nested expressions in a single VALUES ─────────

#[tokio::test]
async fn test_insert_multiple_products() {
    let db = setup().await;

    // Build each row as a nested expression
    let row1 = sqlite_expr!(
        "({}, {}, {}, {}, {}, {})",
        "tart",
        "Time Tart",
        220i64,
        200i64,
        false,
        20i64
    );
    let row2 = sqlite_expr!(
        "({}, {}, {}, {}, {}, {})",
        "donut",
        "DeLorean Doughnut",
        135i64,
        250i64,
        false,
        30i64
    );
    let row3 = sqlite_expr!(
        "({}, {}, {}, {}, {}, {})",
        "pie",
        "Sea Pie",
        299i64,
        350i64,
        true,
        0i64
    );

    // Combine into one INSERT with nested row expressions
    let rows_expr = Expression::from_vec(vec![row1, row2, row3], ", ");
    let insert = Expression::<AnySqliteType>::new(
        "INSERT INTO \"product\" (\"id\", \"name\", \"price\", \"calories\", \"is_deleted\", \"inventory_stock\") VALUES {}",
        vec![ExpressiveEnum::Nested(rows_expr)],
    );

    db.execute(&insert).await.unwrap();

    // Read back and verify
    let select = sqlite_expr!(
        "SELECT name, price, calories, is_deleted, inventory_stock FROM product ORDER BY price"
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
//
// The point of using sqlite_expr! (AnySqliteType) instead of sql_expr! (JsonValue)
// is that parameters carry variant tags. Let's verify that matters.

#[tokio::test]
async fn test_bool_binds_correctly() {
    let db = setup().await;

    // Insert with bool = true → should bind as INTEGER 1
    let insert = sqlite_expr!(
        "INSERT INTO \"product\" (\"id\", \"name\", \"price\", \"calories\", \"is_deleted\", \"inventory_stock\") VALUES ({}, {}, {}, {}, {}, {})",
        "deleted_item",
        "Gone",
        100i64,
        100i64,
        true,
        0i64
    );
    db.execute(&insert).await.unwrap();

    // Query with bool parameter — the type marker ensures it binds as bool, not as string "true"
    let select = sqlite_expr!("SELECT name FROM product WHERE is_deleted = {}", true);
    let result = rows(db.execute(&select).await.unwrap());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["name"], "Gone");
}

#[tokio::test]
async fn test_integer_vs_text_binding() {
    let db = setup().await;

    // Insert a product
    let insert = sqlite_expr!(
        "INSERT INTO \"product\" (\"id\", \"name\", \"price\", \"calories\", \"is_deleted\", \"inventory_stock\") VALUES ({}, {}, {}, {}, {}, {})",
        "item1",
        "Test",
        100i64,
        100i64,
        false,
        10i64
    );
    db.execute(&insert).await.unwrap();

    // Query by integer — Integer variant binds as i64
    let by_price = sqlite_expr!("SELECT id FROM product WHERE price = {}", 100i64);
    let result = rows(db.execute(&by_price).await.unwrap());
    assert_eq!(result.len(), 1);

    // Query by text — Text variant binds as &str
    let by_name = sqlite_expr!("SELECT id FROM product WHERE name = {}", "Test");
    let result = rows(db.execute(&by_name).await.unwrap());
    assert_eq!(result.len(), 1);
}
