//! Test 3a: SqliteSelect via Selectable trait + SelectableDataSource execution.
//!
//! All queries built using the Selectable trait methods, not custom builders.

use vantage_expressions::{ExprDataSource, Expressive, Order, Selectable};
#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::statements::SqliteSelect;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;
use vantage_types::{Record, TryFromRecord, entity};

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    sqlx::query(
        "CREATE TABLE product (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            price INTEGER NOT NULL,
            is_deleted BOOLEAN NOT NULL DEFAULT 0
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    let insert = sqlite_expr!(
        "INSERT INTO product VALUES ({}, {}, {}, {}), ({}, {}, {}, {}), ({}, {}, {}, {})",
        "a",
        "Cheap",
        50i64,
        false,
        "b",
        "Mid",
        150i64,
        false,
        "c",
        "Expensive",
        300i64,
        true
    );
    db.execute(&insert).await.unwrap();

    db
}

#[entity(SqliteType)]
struct Product {
    id: String,
    name: String,
    price: i64,
    is_deleted: bool,
}

// ── Rendering via Selectable trait methods ──────────────────────────────────

#[test]
fn test_select_all() {
    let s = SqliteSelect::new().with_source("product");
    assert_eq!(s.preview(), "SELECT * FROM \"product\"");
}

#[test]
fn test_select_fields() {
    let s = SqliteSelect::new()
        .with_source("product")
        .with_field("name")
        .with_field("price");
    assert_eq!(s.preview(), "SELECT \"name\", \"price\" FROM \"product\"");
}

#[test]
fn test_select_with_condition() {
    let s = SqliteSelect::new()
        .with_source("product")
        .with_condition(sqlite_expr!("\"price\" > {}", 100i64));
    assert_eq!(
        s.preview(),
        "SELECT * FROM \"product\" WHERE \"price\" > 100"
    );
}

#[test]
fn test_select_order_and_limit() {
    let s = SqliteSelect::new()
        .with_source("product")
        .with_order(sqlite_expr!("\"price\""), Order::Desc)
        .with_limit(Some(2), None);
    assert_eq!(
        s.preview(),
        "SELECT * FROM \"product\" ORDER BY \"price\" DESC LIMIT 2"
    );
}

#[test]
fn test_select_distinct() {
    let mut s = SqliteSelect::new()
        .with_source("product")
        .with_field("name");
    s.set_distinct(true);
    assert_eq!(s.preview(), "SELECT DISTINCT \"name\" FROM \"product\"");
}

#[test]
fn test_select_group_by_with_expression() {
    let s = SqliteSelect::new()
        .with_source("product")
        .with_field("is_deleted")
        .with_expression(sqlite_expr!("COUNT(*)"), Some("cnt".to_string()));
    let mut s = s;
    s.add_group_by(sqlite_expr!("\"is_deleted\""));
    assert_eq!(
        s.preview(),
        "SELECT \"is_deleted\", COUNT(*) AS \"cnt\" FROM \"product\" GROUP BY \"is_deleted\""
    );
}

#[test]
fn test_as_count() {
    let s = SqliteSelect::new()
        .with_source("product")
        .with_condition(sqlite_expr!("\"is_deleted\" = {}", false));
    let count_expr = s.as_count();
    assert_eq!(
        count_expr.preview(),
        "SELECT COUNT(*) FROM \"product\" WHERE \"is_deleted\" = 0"
    );
}

#[test]
fn test_as_sum() {
    let s = SqliteSelect::new().with_source("product");
    let sum_expr = s.as_sum(sqlite_expr!("\"price\""));
    assert_eq!(sum_expr.preview(), "SELECT SUM(\"price\") FROM \"product\"");
}

// ── Live execution via ExprDataSource ──────────────────────────────────────

#[tokio::test]
async fn test_execute_select_all() {
    let db = setup().await;

    let select = SqliteSelect::new().with_source("product");
    let result: Record<AnySqliteType> = db.associate(select.expr()).get().await.unwrap();

    // First row — untyped record from DB
    assert!(result.get("id").is_some());
}

#[tokio::test]
async fn test_execute_select_with_condition() {
    let db = setup().await;

    let select = SqliteSelect::new()
        .with_source("product")
        .with_condition(sqlite_expr!("\"is_deleted\" = {}", false));

    let result = db.execute(&select.expr()).await.unwrap();
    let rows = result.into_value();
    assert_eq!(rows.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_execute_count() {
    let db = setup().await;

    let select = SqliteSelect::new()
        .with_source("product")
        .with_condition(sqlite_expr!("\"is_deleted\" = {}", false));
    let count = db.associate::<i64>(select.as_count()).get().await.unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_execute_sum() {
    let db = setup().await;

    let select = SqliteSelect::new().with_source("product");
    let total = db
        .associate::<i64>(select.as_sum(sqlite_expr!("\"price\"")))
        .get()
        .await
        .unwrap();
    assert_eq!(total, 500); // 50 + 150 + 300
}

#[tokio::test]
async fn test_execute_into_entity() {
    let db = setup().await;

    let select = SqliteSelect::new()
        .with_source("product")
        .with_condition(sqlite_expr!("\"id\" = {}", "b"));

    let record: Record<AnySqliteType> = db.associate(select.expr()).get().await.unwrap();
    let product = Product::from_record(record).unwrap();
    assert_eq!(product.name, "Mid");
    assert_eq!(product.price, 150);
    assert!(!product.is_deleted);
}

#[tokio::test]
async fn test_execute_order_and_limit() {
    let db = setup().await;

    let select = SqliteSelect::new()
        .with_source("product")
        .with_order(sqlite_expr!("\"price\""), Order::Desc)
        .with_limit(Some(1), None);

    let record: Record<AnySqliteType> = db.associate(select.expr()).get().await.unwrap();
    let product = Product::from_record(record).unwrap();
    assert_eq!(product.name, "Expensive");
    assert_eq!(product.price, 300);
}
