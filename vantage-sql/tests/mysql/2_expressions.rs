//! Test 2: ExprDataSource — execute Expression<AnyMysqlType> against live MySQL.

use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::mysql::{AnyMysqlType, MysqlDB};
use vantage_types::Record;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

async fn setup(table: &str) -> MysqlDB {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();

    sqlx::query(&format!("DROP TABLE IF EXISTS `{}`", table))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE `{}` (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            price BIGINT NOT NULL,
            weight DOUBLE NOT NULL,
            active BOOLEAN NOT NULL
        )",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO `{}` VALUES (1, 'Apple', 100, 0.2, true), (2, 'Banana', 50, 0.15, true), (3, 'Cherry', 200, 0.01, false)",
        table
    ))
    .execute(db.pool())
    .await
    .unwrap();

    db
}

/// Helper: unwrap result into Vec of Record<AnyMysqlType>.
fn records(result: AnyMysqlType) -> Vec<Record<AnyMysqlType>> {
    Vec::<Record<AnyMysqlType>>::try_from(result).unwrap()
}

// ── Interval renders as INTERVAL <amount> <UNIT> ───────────────────────────

#[test]
fn test_interval_renders_interval_literal() {
    use vantage_expressions::Expressive;
    use vantage_sql::primitives::interval::Interval;

    let thirty_days: Expression<AnyMysqlType> = Interval::new(30, "day").expr();
    assert_eq!(thirty_days.preview(), "INTERVAL 30 DAY");
    let one_month: Expression<AnyMysqlType> = Interval::new(1, "month").expr();
    assert_eq!(one_month.preview(), "INTERVAL 1 MONTH");
}

// ── Scalar parameters, top level and nested ───────────────────────────────

#[tokio::test]
async fn test_scalar_parameters() {
    let db = setup("expr_params").await;

    let price_above = Expression::<AnyMysqlType>::new(
        "price > {}",
        vec![ExpressiveEnum::Scalar(AnyMysqlType::new(75i64))],
    );
    // (price > 75 AND active = true) OR name = 'Cherry'
    let expr = Expression::<AnyMysqlType>::new(
        "SELECT name FROM `expr_params` WHERE {} AND active = {} OR name = {} ORDER BY name",
        vec![
            ExpressiveEnum::Nested(price_above),
            ExpressiveEnum::Scalar(AnyMysqlType::new(true)),
            ExpressiveEnum::Scalar(AnyMysqlType::new("Cherry".to_string())),
        ],
    );
    let result = records(db.execute(&expr).await.unwrap());

    assert_eq!(result.len(), 2);
    assert_eq!(
        result[0]["name"].try_get::<String>(),
        Some("Apple".to_string())
    );
    assert_eq!(
        result[1]["name"].try_get::<String>(),
        Some("Cherry".to_string())
    );
}

// ── Empty result ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_empty_result() {
    let db = setup("expr_empty").await;
    let expr = Expression::<AnyMysqlType>::new(
        "SELECT name FROM `expr_empty` WHERE id = {}",
        vec![ExpressiveEnum::Scalar(AnyMysqlType::new(999i64))],
    );
    let result = records(db.execute(&expr).await.unwrap());

    assert!(result.is_empty());
}

// ── A failed query reports its SQL, capped, with `?` placeholders ──────────

#[tokio::test]
async fn test_failed_query_error_shows_question_placeholders() {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
    let expr = Expression::<AnyMysqlType>::new(
        "SELECT * FROM `no_such_table` WHERE a = {} AND b = {}",
        vec![
            ExpressiveEnum::Scalar(AnyMysqlType::new(1i64)),
            ExpressiveEnum::Scalar(AnyMysqlType::new(2i64)),
        ],
    );
    let msg = db.execute(&expr).await.unwrap_err().to_string();

    assert!(msg.contains("a = ? AND b = ?"), "{msg}");
}

#[tokio::test]
async fn test_failed_query_error_caps_long_sql() {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
    let padding = "x".repeat(5000);
    let expr = Expression::<AnyMysqlType>::new(
        format!("SELECT * FROM `no_such_table` /* {padding} */"),
        vec![],
    );
    let msg = db.execute(&expr).await.unwrap_err().to_string();

    assert!(msg.contains("no_such_table"), "{msg}");
    assert!(!msg.contains(&padding), "the whole statement was echoed");
}
