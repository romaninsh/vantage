//! Test 2d: AssociatedExpression — expressions with a known return type.
//!
//! Call .get() to execute and get a typed result.

use vantage_expressions::ExprDataSource;
#[allow(unused_imports)]
use vantage_sql::mysql::MysqlType;
use vantage_sql::mysql::{AnyMysqlType, MysqlDB};
use vantage_sql::mysql_expr;
use vantage_types::{Record, TryFromRecord, entity};

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
            price BIGINT NOT NULL
        )",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO `{}` VALUES ('a', 'Cheap', 50), ('b', 'Mid', 150), ('c', 'Expensive', 300)",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    db
}

#[entity(MysqlType)]
struct Product {
    id: String,
    name: String,
    price: i64,
}

#[tokio::test]
async fn test_associated_scalar() {
    let db = setup("assoc_scalar").await;

    let associated = db.associate::<i64>(mysql_expr!("SELECT COUNT(*) FROM `assoc_scalar`"));
    assert_eq!(associated.get().await.unwrap(), 3);
}

#[tokio::test]
async fn test_associated_record() {
    let db = setup("assoc_record").await;

    let associated = db.associate::<Record<AnyMysqlType>>(mysql_expr!(
        "SELECT name, price FROM `assoc_record` WHERE id = {}",
        "a"
    ));
    let record = associated.get().await.unwrap();
    assert_eq!(
        record["name"].try_get::<String>(),
        Some("Cheap".to_string())
    );
    assert_eq!(record["price"].try_get::<i64>(), Some(50));
}

#[tokio::test]
async fn test_associated_entity() {
    let db = setup("assoc_entity").await;

    let associated = db.associate::<Record<AnyMysqlType>>(mysql_expr!(
        "SELECT id, name, price FROM `assoc_entity` WHERE id = {}",
        "c"
    ));
    let record = associated.get().await.unwrap();
    let product = Product::from_record(record).unwrap();
    assert_eq!(product.id, "c");
    assert_eq!(product.name, "Expensive");
    assert_eq!(product.price, 300);
}

// serde path: Record<JsonValue> + #[derive(Deserialize)]
#[tokio::test]
async fn test_associated_entity_serde() {
    let db = setup("assoc_serde").await;

    #[derive(serde::Deserialize)]
    struct ProductSerde {
        id: String,
        name: String,
        price: i64,
    }

    let record: Record<serde_json::Value> = db
        .associate(mysql_expr!(
            "SELECT id, name, price FROM `assoc_serde` WHERE id = {}",
            "b"
        ))
        .get()
        .await
        .unwrap();
    let product: ProductSerde = ProductSerde::from_record(record).unwrap();
    assert_eq!(product.id, "b");
    assert_eq!(product.name, "Mid");
    assert_eq!(product.price, 150);
}
