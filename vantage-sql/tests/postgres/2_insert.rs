//! Test 2a: INSERT via Expression<AnyPostgresType> + ExprDataSource.
//!
//! No statement builders — raw expressions with typed parameters.
//! Focuses on the Product table to exercise multiple types (text, integer, bool).

use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use vantage_expressions::Expressive;
use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
use vantage_sql::postgres_expr;
use vantage_types::{Record, TryFromRecord};

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
struct Product {
    name: String,
    price: i64,
    calories: i64,
    is_deleted: bool,
    inventory_stock: i64,
}

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

/// Helper: unwrap result into Vec of Record<AnyPostgresType>.
fn records(result: AnyPostgresType) -> Vec<Record<AnyPostgresType>> {
    Vec::<Record<AnyPostgresType>>::try_from(result).unwrap()
}

// ── Insert multiple rows via nested expressions ───────────────────────────

#[tokio::test]
async fn test_insert_multiple_products() {
    let db = setup("ins_multi").await;

    let row1 = postgres_expr!(
        "({}, {}, {}, {}, {}, {})",
        "tart",
        "Time Tart",
        220i64,
        200i64,
        false,
        20i64
    );
    let row2 = postgres_expr!(
        "({}, {}, {}, {}, {}, {})",
        "donut",
        "DeLorean Doughnut",
        135i64,
        250i64,
        false,
        30i64
    );
    let row3 = postgres_expr!(
        "({}, {}, {}, {}, {}, {})",
        "pie",
        "Sea Pie",
        299i64,
        350i64,
        true,
        0i64
    );

    let rows_expr = Expression::from_vec(vec![row1, row2, row3], ", ");
    let insert = Expression::<AnyPostgresType>::new(
        "INSERT INTO \"ins_multi\" (\"id\", \"name\", \"price\", \"calories\", \"is_deleted\", \"inventory_stock\") VALUES {}",
        vec![ExpressiveEnum::Nested(rows_expr)],
    );

    db.execute(&insert).await.unwrap();

    let select = postgres_expr!(
        "SELECT name, price, calories, is_deleted, inventory_stock FROM \"ins_multi\" ORDER BY price"
    );
    let result = records(db.execute(&select).await.unwrap());

    assert_eq!(result.len(), 3);

    assert_eq!(
        result[0]["name"].try_get::<String>(),
        Some("DeLorean Doughnut".to_string())
    );
    assert_eq!(result[0]["price"].try_get::<i64>(), Some(135));
    assert_eq!(result[0]["calories"].try_get::<i64>(), Some(250));
    assert_eq!(result[0]["is_deleted"].try_get::<bool>(), Some(false));

    assert_eq!(
        result[2]["name"].try_get::<String>(),
        Some("Sea Pie".to_string())
    );
    assert_eq!(result[2]["price"].try_get::<i64>(), Some(299));
    assert_eq!(result[2]["is_deleted"].try_get::<bool>(), Some(true));
    assert_eq!(result[2]["inventory_stock"].try_get::<i64>(), Some(0));
}

// ── Insert + round-trip via serde deserialization ──────────────────────────

#[tokio::test]
async fn test_insert_and_deserialize_product() {
    let db = setup("ins_serde").await;

    let insert = postgres_expr!(
        "INSERT INTO \"ins_serde\" (\"id\", \"name\", \"price\", \"calories\", \"is_deleted\", \"inventory_stock\") VALUES ({}, {}, {}, {}, {}, {})",
        "cupcake",
        "Flux Cupcake",
        120i64,
        300i64,
        false,
        50i64
    );
    db.execute(&insert).await.unwrap();

    let select = postgres_expr!(
        "SELECT name, price, calories, is_deleted, inventory_stock FROM \"ins_serde\" WHERE id = {}",
        "cupcake"
    );

    // Serde path: AnyPostgresType → JsonValue → Record<JsonValue> → Product
    let json: serde_json::Value = db.execute(&select).await.unwrap().into();
    let arr = json.as_array().unwrap();
    let record: Record<serde_json::Value> = arr[0].clone().into();
    let product: Product = Product::from_record(record).unwrap();

    assert_eq!(product.name, "Flux Cupcake");
    assert_eq!(product.price, 120);
    assert_eq!(product.calories, 300);
    assert!(!product.is_deleted);
    assert_eq!(product.inventory_stock, 50);
}
