//! Test 3a: PostgresSelect via Selectable trait + SelectableDataSource execution.

use vantage_expressions::{ExprDataSource, Expressive, Selectable};
#[allow(unused_imports)]
use vantage_sql::postgres::PostgresType;
use vantage_sql::postgres::statements::PostgresSelect;
use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
use vantage_sql::postgres_expr;
use vantage_types::{Record, TryFromRecord, entity};

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

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
            is_deleted BOOLEAN NOT NULL DEFAULT false
        )",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO \"{}\" VALUES ('a', 'Cheap', 50, false), ('b', 'Mid', 150, false), ('c', 'Expensive', 300, true)",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    db
}

#[entity(PostgresType)]
struct Product {
    id: String,
    name: String,
    price: i64,
    is_deleted: bool,
}

// ── Rendering via Selectable trait methods ──────────────────────────────────

#[test]
fn test_select_all() {
    let s = PostgresSelect::new().with_source("product");
    assert_eq!(s.preview(), "SELECT * FROM \"product\"");
}

#[test]
fn test_select_fields() {
    let s = PostgresSelect::new()
        .with_source("product")
        .with_field("name")
        .with_field("price");
    assert_eq!(s.preview(), "SELECT \"name\", \"price\" FROM \"product\"");
}

#[test]
fn test_select_with_condition() {
    let s = PostgresSelect::new()
        .with_source("product")
        .with_condition(postgres_expr!("\"price\" > {}", 100i64));
    assert_eq!(
        s.preview(),
        "SELECT * FROM \"product\" WHERE \"price\" > 100"
    );
}

#[test]
fn test_select_order_and_limit() {
    let s = PostgresSelect::new()
        .with_source("product")
        .with_order(postgres_expr!("\"price\""), false)
        .with_limit(Some(2), None);
    assert_eq!(
        s.preview(),
        "SELECT * FROM \"product\" ORDER BY \"price\" DESC LIMIT 2"
    );
}

#[test]
fn test_select_distinct() {
    let mut s = PostgresSelect::new()
        .with_source("product")
        .with_field("name");
    s.set_distinct(true);
    assert_eq!(s.preview(), "SELECT DISTINCT \"name\" FROM \"product\"");
}

#[test]
fn test_select_group_by_with_expression() {
    let s = PostgresSelect::new()
        .with_source("product")
        .with_field("is_deleted")
        .with_expression(postgres_expr!("COUNT(*)"), Some("cnt".to_string()));
    let mut s = s;
    s.add_group_by(postgres_expr!("\"is_deleted\""));
    assert_eq!(
        s.preview(),
        "SELECT \"is_deleted\", COUNT(*) AS \"cnt\" FROM \"product\" GROUP BY \"is_deleted\""
    );
}

#[test]
fn test_as_count() {
    let s = PostgresSelect::new()
        .with_source("product")
        .with_condition(postgres_expr!("\"is_deleted\" = {}", false));
    let count_expr = s.as_count();
    assert_eq!(
        count_expr.preview(),
        "SELECT COUNT(*) FROM \"product\" WHERE \"is_deleted\" = false"
    );
}

#[test]
fn test_as_sum() {
    let s = PostgresSelect::new().with_source("product");
    let sum_expr = s.as_sum(postgres_expr!("\"price\""));
    assert_eq!(
        sum_expr.preview(),
        "SELECT CAST(SUM(\"price\") AS BIGINT) FROM \"product\""
    );
}

// ── Live execution via ExprDataSource ──────────────────────────────────────

#[tokio::test]
async fn test_execute_select_all() {
    let db = setup("sel_all").await;

    let select = PostgresSelect::new().with_source("sel_all");
    let result: Record<AnyPostgresType> = db.associate(select.expr()).get().await.unwrap();

    assert!(result.get("id").is_some());
}

#[tokio::test]
async fn test_execute_select_with_condition() {
    let db = setup("sel_cond").await;

    let select = PostgresSelect::new()
        .with_source("sel_cond")
        .with_condition(postgres_expr!("\"is_deleted\" = {}", false));

    let result = db.execute(&select.expr()).await.unwrap();
    let rows = result.into_value();
    assert_eq!(rows.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_execute_count() {
    let db = setup("sel_count").await;

    let select = PostgresSelect::new()
        .with_source("sel_count")
        .with_condition(postgres_expr!("\"is_deleted\" = {}", false));
    let count = db.associate::<i64>(select.as_count()).get().await.unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_execute_sum() {
    let db = setup("sel_sum").await;

    let select = PostgresSelect::new().with_source("sel_sum");
    let total = db
        .associate::<i64>(select.as_sum(postgres_expr!("\"price\"")))
        .get()
        .await
        .unwrap();
    assert_eq!(total, 500); // 50 + 150 + 300
}

#[tokio::test]
async fn test_execute_into_entity() {
    let db = setup("sel_entity").await;

    let select = PostgresSelect::new()
        .with_source("sel_entity")
        .with_condition(postgres_expr!("\"id\" = {}", "b"));

    let record: Record<AnyPostgresType> = db.associate(select.expr()).get().await.unwrap();
    let product = Product::from_record(record).unwrap();
    assert_eq!(product.name, "Mid");
    assert_eq!(product.price, 150);
    assert!(!product.is_deleted);
}

#[tokio::test]
async fn test_execute_order_and_limit() {
    let db = setup("sel_order").await;

    let select = PostgresSelect::new()
        .with_source("sel_order")
        .with_order(postgres_expr!("\"price\""), false)
        .with_limit(Some(1), None);

    let record: Record<AnyPostgresType> = db.associate(select.expr()).get().await.unwrap();
    let product = Product::from_record(record).unwrap();
    assert_eq!(product.name, "Expensive");
    assert_eq!(product.price, 300);
}
