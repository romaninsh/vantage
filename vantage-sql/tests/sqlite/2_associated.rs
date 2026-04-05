//! Test 2d: AssociatedExpression — expressions with a known return type.
//!
//! Call .get() to execute and get a typed result. Works out of the box
//! once ExprDataSource and TryFrom<AnySqliteType> are in place.

use vantage_expressions::ExprDataSource;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite_expr;
use vantage_types::{Record, TryFromRecord, entity};

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    sqlx::query(
        "CREATE TABLE product (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            price INTEGER NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    let insert = sqlite_expr!(
        "INSERT INTO product VALUES ({}, {}, {}), ({}, {}, {}), ({}, {}, {})",
        "a", "Cheap", 50i64,
        "b", "Mid", 150i64,
        "c", "Expensive", 300i64
    );
    db.execute(&insert).await.unwrap();

    db
}

#[entity(SqliteType)]
struct Product {
    id: String,
    name: String,
    price: i64,
}

#[tokio::test]
async fn test_associated_scalar() {
    let db = setup().await;

    let associated = db.associate::<i64>(sqlite_expr!("SELECT COUNT(*) FROM product"));
    assert_eq!(associated.get().await.unwrap(), 3);
}

#[tokio::test]
async fn test_associated_record() {
    let db = setup().await;

    let associated = db.associate::<Record<AnySqliteType>>(
        sqlite_expr!("SELECT name, price FROM product WHERE id = {}", "a"),
    );
    let record = associated.get().await.unwrap();
    assert_eq!(record["name"].try_get::<String>(), Some("Cheap".to_string()));
    assert_eq!(record["price"].try_get::<i64>(), Some(50));
}

#[tokio::test]
async fn test_associated_entity() {
    let db = setup().await;

    let associated = db.associate::<Record<AnySqliteType>>(
        sqlite_expr!("SELECT id, name, price FROM product WHERE id = {}", "c"),
    );
    let record = associated.get().await.unwrap();
    let product = Product::from_record(record).unwrap();
    assert_eq!(product.id, "c");
    assert_eq!(product.name, "Expensive");
    assert_eq!(product.price, 300);
}

// serde path: Record<JsonValue> + #[derive(Deserialize)]
#[tokio::test]
async fn test_associated_entity_serde() {
    let db = setup().await;

    #[derive(serde::Deserialize)]
    struct ProductSerde {
        id: String,
        name: String,
        price: i64,
    }

    let record: Record<serde_json::Value> = db
        .associate(sqlite_expr!("SELECT id, name, price FROM product WHERE id = {}", "b"))
        .get()
        .await
        .unwrap();
    let product: ProductSerde = ProductSerde::from_record(record).unwrap();
    assert_eq!(product.id, "b");
    assert_eq!(product.name, "Mid");
    assert_eq!(product.price, 150);
}
