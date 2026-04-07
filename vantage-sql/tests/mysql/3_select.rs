//! Test 3a: MysqlSelect via Selectable trait + SelectableDataSource execution.

use vantage_expressions::{ExprDataSource, Expressive, Order, Selectable};
#[allow(unused_imports)]
use vantage_sql::mysql::MysqlType;
use vantage_sql::mysql::statements::MysqlSelect;
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
            price BIGINT NOT NULL,
            is_deleted BOOLEAN NOT NULL DEFAULT false
        )",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO `{}` VALUES ('a', 'Cheap', 50, false), ('b', 'Mid', 150, false), ('c', 'Expensive', 300, true)",
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
    is_deleted: bool,
}

// ── Rendering via Selectable trait methods ──────────────────────────────────

#[test]
fn test_select_all() {
    let s = MysqlSelect::new().with_source("product");
    assert_eq!(s.preview(), "SELECT * FROM `product`");
}

#[test]
fn test_select_fields() {
    let s = MysqlSelect::new()
        .with_source("product")
        .with_field("name")
        .with_field("price");
    assert_eq!(s.preview(), "SELECT `name`, `price` FROM `product`");
}

#[test]
fn test_select_with_condition() {
    let s = MysqlSelect::new()
        .with_source("product")
        .with_condition(mysql_expr!("`price` > {}", 100i64));
    assert_eq!(s.preview(), "SELECT * FROM `product` WHERE `price` > 100");
}

#[test]
fn test_select_order_and_limit() {
    let s = MysqlSelect::new()
        .with_source("product")
        .with_order(mysql_expr!("`price`"), Order::Desc)
        .with_limit(Some(2), None);
    assert_eq!(
        s.preview(),
        "SELECT * FROM `product` ORDER BY `price` DESC LIMIT 2"
    );
}

#[test]
fn test_select_distinct() {
    let mut s = MysqlSelect::new().with_source("product").with_field("name");
    s.set_distinct(true);
    assert_eq!(s.preview(), "SELECT DISTINCT `name` FROM `product`");
}

#[test]
fn test_select_group_by_with_expression() {
    let s = MysqlSelect::new()
        .with_source("product")
        .with_field("is_deleted")
        .with_expression(mysql_expr!("COUNT(*)"), Some("cnt".to_string()));
    let mut s = s;
    s.add_group_by(mysql_expr!("`is_deleted`"));
    assert_eq!(
        s.preview(),
        "SELECT `is_deleted`, COUNT(*) AS `cnt` FROM `product` GROUP BY `is_deleted`"
    );
}

#[test]
fn test_as_count() {
    let s = MysqlSelect::new()
        .with_source("product")
        .with_condition(mysql_expr!("`is_deleted` = {}", false));
    let count_expr = s.as_count();
    assert_eq!(
        count_expr.preview(),
        "SELECT COUNT(*) FROM `product` WHERE `is_deleted` = false"
    );
}

#[test]
fn test_as_sum() {
    let s = MysqlSelect::new().with_source("product");
    let sum_expr = s.as_sum(mysql_expr!("`price`"));
    assert_eq!(
        sum_expr.preview(),
        "SELECT CAST(SUM(`price`) AS SIGNED) FROM `product`"
    );
}

// ── Live execution via ExprDataSource ──────────────────────────────────────

#[tokio::test]
async fn test_execute_select_all() {
    let db = setup("sel_all").await;

    let select = MysqlSelect::new().with_source("sel_all");
    let result: Record<AnyMysqlType> = db.associate(select.expr()).get().await.unwrap();

    assert!(result.get("id").is_some());
}

#[tokio::test]
async fn test_execute_select_with_condition() {
    let db = setup("sel_cond").await;

    let select = MysqlSelect::new()
        .with_source("sel_cond")
        .with_condition(mysql_expr!("`is_deleted` = {}", false));

    let result = db.execute(&select.expr()).await.unwrap();
    let rows = result.into_value();
    assert_eq!(rows.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_execute_count() {
    let db = setup("sel_count").await;

    let select = MysqlSelect::new()
        .with_source("sel_count")
        .with_condition(mysql_expr!("`is_deleted` = {}", false));
    let count = db.associate::<i64>(select.as_count()).get().await.unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_execute_sum() {
    let db = setup("sel_sum").await;

    let select = MysqlSelect::new().with_source("sel_sum");
    let total = db
        .associate::<i64>(select.as_sum(mysql_expr!("`price`")))
        .get()
        .await
        .unwrap();
    assert_eq!(total, 500); // 50 + 150 + 300
}

#[tokio::test]
async fn test_execute_into_entity() {
    let db = setup("sel_entity").await;

    let select = MysqlSelect::new()
        .with_source("sel_entity")
        .with_condition(mysql_expr!("`id` = {}", "b"));

    let record: Record<AnyMysqlType> = db.associate(select.expr()).get().await.unwrap();
    let product = Product::from_record(record).unwrap();
    assert_eq!(product.name, "Mid");
    assert_eq!(product.price, 150);
    assert!(!product.is_deleted);
}

#[tokio::test]
async fn test_execute_order_and_limit() {
    let db = setup("sel_order").await;

    let select = MysqlSelect::new()
        .with_source("sel_order")
        .with_order(mysql_expr!("`price`"), Order::Desc)
        .with_limit(Some(1), None);

    let record: Record<AnyMysqlType> = db.associate(select.expr()).get().await.unwrap();
    let product = Product::from_record(record).unwrap();
    assert_eq!(product.name, "Expensive");
    assert_eq!(product.price, 300);
}
