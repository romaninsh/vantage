//! Decimal type matrix test via Table<PostgresDB, Entity>.
//!
//! rust_decimal::Decimal × 6 column types.
//! Tables are pre-created by v5.sql (same shape: id, name, value).
//! Postgres may reject text binds for typed columns (NUMERIC, etc).

use rust_decimal::Decimal;
use std::str::FromStr;

#[allow(unused_imports)]
use vantage_sql::postgres::PostgresType;
use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::{ReadableDataSet, WritableDataSet};

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage_v5";

async fn db() -> PostgresDB {
    PostgresDB::connect(PG_URL).await.unwrap()
}

#[entity(PostgresType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValDecimal {
    name: String,
    value: Decimal,
}

macro_rules! table_for {
    ($table:expr, $db:expr) => {
        Table::<PostgresDB, ValDecimal>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<Decimal>("value")
    };
}

async fn try_round_trip(
    table: &Table<PostgresDB, ValDecimal>,
    id: &str,
    entity: &ValDecimal,
) -> Result<ValDecimal, String> {
    table
        .replace(&id.to_string(), entity)
        .await
        .map_err(|e| e.to_string())?;
    table
        .get(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "row missing after replace".to_string())
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

// ═════════════════════════════════════════════════════════════════════════
// 1. VARCHAR — exact string round-trip
// ══════════════════════════════════════════════════════════════════════���══

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
// 2. NUMERIC(20,6) — precision limited to 6 decimal places
// ════════════════════════════════════════════════════════���════════════════

#[tokio::test]
async fn test_numeric_exact() {
    let t = table_for!("decimal_numeric", db().await);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("123456.789012"),
    };
    let fetched = try_round_trip(&t, "dn_exact", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_numeric_truncated() {
    // More than 6 decimal places — Postgres rounds
    let t = table_for!("decimal_numeric", db().await);
    let orig = ValDecimal {
        name: "trunc".into(),
        value: dec("123.123456789"),
    };
    let fetched = try_round_trip(&t, "dn_trunc", &orig).await.unwrap();
    assert_eq!(fetched.value, dec("123.123457"));
}

#[tokio::test]
async fn test_numeric_integer() {
    let t = table_for!("decimal_numeric", db().await);
    let orig = ValDecimal {
        name: "int".into(),
        value: dec("42"),
    };
    let fetched = try_round_trip(&t, "dn_int", &orig).await.unwrap();
    assert_eq!(fetched.value, dec("42.000000"));
}

// ═════════════════════════════════════════════════════════════════════════
// 2b. NUMERIC(38,15) — wide enough for full rust_decimal precision
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_numeric_wide_high_precision() {
    let t = table_for!("decimal_numeric_wide", db().await);
    let orig = ValDecimal {
        name: "hp".into(),
        value: dec("99999999999999999.123456789012345"),
    };
    let fetched = try_round_trip(&t, "dnw_hp", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_numeric_wide_negative() {
    let t = table_for!("decimal_numeric_wide", db().await);
    let orig = ValDecimal {
        name: "neg".into(),
        value: dec("-12345678901234.567890123456789"),
    };
    let fetched = try_round_trip(&t, "dnw_neg", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_numeric_wide_tiny() {
    let t = table_for!("decimal_numeric_wide", db().await);
    let orig = ValDecimal {
        name: "tiny".into(),
        value: dec("0.000000000000001"),
    };
    let fetched = try_round_trip(&t, "dnw_tiny", &orig).await.unwrap();
    assert_eq!(fetched, orig);
}

// ═══════════════════════════════════════════════════════��═════════════════
// 3. DOUBLE PRECISION — f64 precision (~15 significant digits)
// ═══════════════════════════════════════════════════════════��═════════════

#[tokio::test]
async fn test_double_simple() {
    let t = table_for!("decimal_double", db().await);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("123.456"),
    };
    let fetched = try_round_trip(&t, "dbl_s", &orig).await.unwrap();
    let diff = (fetched.value - orig.value).abs();
    assert!(diff < dec("0.001"), "diff too large: {}", diff);
}

// ═════════════════════════════════════════════════════════════════════════
// 4. REAL — f32 precision (~7 significant digits)
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_real_simple() {
    let t = table_for!("decimal_real", db().await);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("123.456"),
    };
    let fetched = try_round_trip(&t, "real_s", &orig).await.unwrap();
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
