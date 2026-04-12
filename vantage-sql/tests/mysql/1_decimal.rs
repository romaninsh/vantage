//! Numeric type matrix tests via Table<MysqlDB, Entity>.
//!
//! Tests Decimal, i64, and f64 across column types (VARCHAR, DECIMAL, DOUBLE,
//! FLOAT, BIGINT). Tables pre-created by v5.sql (same shape: id, name, value).

use rust_decimal::Decimal;
use std::str::FromStr;

#[allow(unused_imports)]
use vantage_sql::mysql::MysqlType;
use vantage_sql::mysql::{AnyMysqlType, MysqlDB};
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::{ReadableDataSet, WritableDataSet};

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage_v5";

async fn db() -> MysqlDB {
    MysqlDB::connect(MYSQL_URL).await.unwrap()
}

#[entity(MysqlType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValDecimal {
    name: String,
    value: Decimal,
}

macro_rules! table_for {
    ($table:expr, $db:expr) => {
        Table::<MysqlDB, ValDecimal>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<Decimal>("value")
    };
}

/// Try replace + read-back.
async fn try_round_trip(
    table: &Table<MysqlDB, ValDecimal>,
    id: &str,
    entity: &ValDecimal,
) -> Result<ValDecimal, String> {
    table
        .replace(&id.to_string(), entity)
        .await
        .map_err(|e| e.to_string())?;
    table.get(id).await.map_err(|e| e.to_string())
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

// ═════════════════════════════════════════════════════════════════════════
// 1. VARCHAR — exact string round-trip
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_varchar_decimal() {
    let t = table_for!("decimal_varchar", db().await);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("123456.789012"),
    };
    let fetched = try_round_trip(&t, "vc_d", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_varchar_high_precision() {
    let t = table_for!("decimal_varchar", db().await);
    let orig = ValDecimal {
        name: "hp".into(),
        value: dec("99999999999999999.123456789012345"),
    };
    let fetched = try_round_trip(&t, "vc_hp", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

// ═════════════════════════════════════════════════════════════════════════
// 2. DECIMAL(20,6) — precision limited to 6 decimal places
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_decimal_exact() {
    let t = table_for!("decimal_decimal", db().await);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("123456.789012"),
    };
    let fetched = try_round_trip(&t, "dd_exact", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_decimal_truncated() {
    // More than 6 decimal places — MySQL truncates
    let t = table_for!("decimal_decimal", db().await);
    let orig = ValDecimal {
        name: "trunc".into(),
        value: dec("123.123456789"),
    };
    let fetched = try_round_trip(&t, "dd_trunc", &orig).await.unwrap();
    // MySQL DECIMAL(20,6) rounds to 6 places
    assert_eq!(fetched.value, dec("123.123457"));
}

#[tokio::test]
async fn test_decimal_integer() {
    let t = table_for!("decimal_decimal", db().await);
    let orig = ValDecimal {
        name: "int".into(),
        value: dec("42"),
    };
    let fetched = try_round_trip(&t, "dd_int", &orig).await.unwrap();
    // MySQL stores as 42.000000
    assert_eq!(fetched.value, dec("42.000000"));
}

// ═════════════════════════════════════════════════════════════════════════
// 2b. DECIMAL(38,15) — wide enough for full rust_decimal precision
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_decimal_wide_high_precision() {
    let t = table_for!("decimal_decimal_wide", db().await);
    let orig = ValDecimal {
        name: "hp".into(),
        value: dec("99999999999999999.123456789012345"),
    };
    let fetched = try_round_trip(&t, "dw_hp", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_decimal_wide_negative() {
    let t = table_for!("decimal_decimal_wide", db().await);
    let orig = ValDecimal {
        name: "neg".into(),
        value: dec("-12345678901234.567890123456789"),
    };
    let fetched = try_round_trip(&t, "dw_neg", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_decimal_wide_tiny() {
    let t = table_for!("decimal_decimal_wide", db().await);
    let orig = ValDecimal {
        name: "tiny".into(),
        value: dec("0.000000000000001"),
    };
    let fetched = try_round_trip(&t, "dw_tiny", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

// ═════════════════════════════════════════════════════════════════════════
// 3. DOUBLE — f64 precision (~15 significant digits)
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_double_simple() {
    let t = table_for!("decimal_double", db().await);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("123.456"),
    };
    let fetched = try_round_trip(&t, "dbl_s", &orig).await.unwrap();
    // DOUBLE may lose trailing precision
    let diff = (fetched.value - orig.value).abs();
    assert!(diff < dec("0.001"), "diff too large: {}", diff);
}

// ═════════════════════════════════════════════════════════════════════════
// 4. FLOAT — f32 precision (~7 significant digits)
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_float_simple() {
    let t = table_for!("decimal_float", db().await);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("123.456"),
    };
    let fetched = try_round_trip(&t, "flt_s", &orig).await.unwrap();
    // FLOAT has ~7 significant digits
    let diff = (fetched.value - orig.value).abs();
    assert!(diff < dec("0.01"), "diff too large: {}", diff);
}

// ═════════════════════════════════════════════════════════════════════════
// 5. BIGINT — integer only, fractional part lost
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_bigint_integer() {
    let t = table_for!("decimal_bigint", db().await);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("42"),
    };
    let fetched = try_round_trip(&t, "bi_int", &orig).await.unwrap();
    assert_eq!(fetched.value, dec("42"));
}

#[tokio::test]
async fn test_bigint_truncates_fraction() {
    let t = table_for!("decimal_bigint", db().await);
    let orig = ValDecimal {
        name: "frac".into(),
        value: dec("123.999"),
    };
    let fetched = try_round_trip(&t, "bi_frac", &orig).await.unwrap();
    // BIGINT truncates fractional part
    assert_eq!(fetched.value, dec("124"));
}

// ═════════════════════════════════════════════════════════════════════════
// i64 across column types
// ═════════════════════════════════════════════════════════════════════════

#[entity(MysqlType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValI64 {
    name: String,
    value: i64,
}

macro_rules! table_i64 {
    ($table:expr, $db:expr) => {
        Table::<MysqlDB, ValI64>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("value")
    };
}

async fn try_i64(
    table: &Table<MysqlDB, ValI64>,
    id: &str,
    entity: &ValI64,
) -> Result<ValI64, String> {
    table
        .replace(&id.to_string(), entity)
        .await
        .map_err(|e| e.to_string())?;
    table.get(id).await.map_err(|e| e.to_string())
}

#[tokio::test]
async fn test_i64_varchar() {
    let t = table_i64!("decimal_varchar", db().await);
    let orig = ValI64 {
        name: "i".into(),
        value: 42,
    };
    let fetched = try_i64(&t, "i64_vc", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_i64_bigint() {
    let t = table_i64!("decimal_bigint", db().await);
    let orig = ValI64 {
        name: "i".into(),
        value: 9_223_372_036_854_775_807,
    }; // i64::MAX
    let fetched = try_i64(&t, "i64_bi", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_i64_double() {
    // DOUBLE returns Float — no Float→Integer cross-conversion
    let t = table_i64!("decimal_double", db().await);
    let orig = ValI64 {
        name: "i".into(),
        value: 42,
    };
    assert!(try_i64(&t, "i64_dbl", &orig).await.is_err());
}

#[tokio::test]
async fn test_i64_double_large() {
    // i64 beyond f64 exact range (~2^53) — precision lost
    let t = table_i64!("decimal_double", db().await);
    let orig = ValI64 {
        name: "big".into(),
        value: 9_007_199_254_740_993,
    }; // 2^53 + 1
    // DOUBLE returns Float — no Float→Integer cross-conversion
    assert!(try_i64(&t, "i64_dbl_big", &orig).await.is_err());
}

#[tokio::test]
async fn test_i64_decimal() {
    // DECIMAL returns Tag(10, Text) — no Decimal→Integer cross-conversion
    let t = table_i64!("decimal_decimal", db().await);
    let orig = ValI64 {
        name: "i".into(),
        value: 42,
    };
    assert!(try_i64(&t, "i64_dec", &orig).await.is_err());
}

#[tokio::test]
async fn test_i64_float() {
    // FLOAT returns Float — no Float→Integer cross-conversion
    let t = table_i64!("decimal_float", db().await);
    let orig = ValI64 {
        name: "i".into(),
        value: 42,
    };
    assert!(try_i64(&t, "i64_flt", &orig).await.is_err());
}

#[tokio::test]
async fn test_i64_decimal_wide() {
    // DECIMAL(38,15) returns Tag(10, Text) — no Decimal→Integer cross-conversion
    let t = table_i64!("decimal_decimal_wide", db().await);
    let orig = ValI64 {
        name: "i".into(),
        value: 42,
    };
    assert!(try_i64(&t, "i64_dw", &orig).await.is_err());
}

// ═════════════════════════════════════════════════════════════════════════
// f64 across column types
// ═════════════════════════════════════════════════════════════════════════

#[entity(MysqlType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValF64 {
    name: String,
    value: f64,
}

macro_rules! table_f64 {
    ($table:expr, $db:expr) => {
        Table::<MysqlDB, ValF64>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<f64>("value")
    };
}

async fn try_f64(
    table: &Table<MysqlDB, ValF64>,
    id: &str,
    entity: &ValF64,
) -> Result<ValF64, String> {
    table
        .replace(&id.to_string(), entity)
        .await
        .map_err(|e| e.to_string())?;
    table.get(id).await.map_err(|e| e.to_string())
}

#[tokio::test]
async fn test_f64_varchar() {
    let t = table_f64!("decimal_varchar", db().await);
    let orig = ValF64 {
        name: "f".into(),
        value: 123.456,
    };
    let fetched = try_f64(&t, "f64_vc", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_f64_double() {
    let t = table_f64!("decimal_double", db().await);
    let orig = ValF64 {
        name: "f".into(),
        value: 123.456,
    };
    let fetched = try_f64(&t, "f64_dbl", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_f64_float() {
    // FLOAT is f32 — precision loss
    let t = table_f64!("decimal_float", db().await);
    let orig = ValF64 {
        name: "f".into(),
        value: 123.456,
    };
    let fetched = try_f64(&t, "f64_flt", &orig).await.unwrap();
    assert!((fetched.value - orig.value).abs() < 0.01, "diff too large");
}

#[tokio::test]
async fn test_f64_bigint() {
    // BIGINT returns Integer — no Integer→Float cross-conversion
    let t = table_f64!("decimal_bigint", db().await);
    let orig = ValF64 {
        name: "f".into(),
        value: 123.999,
    };
    assert!(try_f64(&t, "f64_bi", &orig).await.is_err());
}

#[tokio::test]
async fn test_f64_decimal() {
    // DECIMAL returns Tag(10, Text) — no Decimal→Float cross-conversion
    let t = table_f64!("decimal_decimal", db().await);
    let orig = ValF64 {
        name: "f".into(),
        value: 123.456,
    };
    assert!(try_f64(&t, "f64_dec", &orig).await.is_err());
}

#[tokio::test]
async fn test_f64_decimal_wide() {
    // DECIMAL(38,15) returns Tag(10, Text) — no Decimal→Float cross-conversion
    let t = table_f64!("decimal_decimal_wide", db().await);
    let orig = ValF64 {
        name: "f".into(),
        value: 123.456,
    };
    assert!(try_f64(&t, "f64_dw", &orig).await.is_err());
}
