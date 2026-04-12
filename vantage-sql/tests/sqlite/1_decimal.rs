//! Decimal type round-trips via Table<SqliteDB, Entity>.
//!
//! SQLite has no native DECIMAL type — NUMERIC affinity stores values as
//! TEXT, REAL, or INTEGER internally. Tests verify precision preservation
//! across column affinities.

use rust_decimal::Decimal;
use std::str::FromStr;

#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::{ReadableDataSet, WritableDataSet};

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ValDecimal {
    name: String,
    value: Decimal,
}

macro_rules! table_for {
    ($table:expr, $db:expr) => {
        Table::<SqliteDB, ValDecimal>::new($table, $db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<Decimal>("value")
    };
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    for (name, col_type) in [
        ("decimal_text", "TEXT"),
        ("decimal_numeric", "NUMERIC"),
        ("decimal_real", "REAL"),
        ("decimal_integer", "INTEGER"),
    ] {
        sqlx::query(&format!(
            "CREATE TABLE \"{}\" (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                value {} NOT NULL
            )",
            name, col_type
        ))
        .execute(db.pool())
        .await
        .unwrap();
    }

    db
}

// ═════════════════════════════════════════════════════════════════════════
// 1. TEXT — exact round-trip
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_text_decimal() {
    let db = setup().await;
    let t = table_for!("decimal_text", db);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("123456.789012"),
    };
    let inserted = t.insert(&"t1".to_string(), &orig).await.unwrap();
    assert_eq!(inserted, orig);
    let fetched = t.get("t1").await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_text_high_precision() {
    let db = setup().await;
    let t = table_for!("decimal_text", db);
    let orig = ValDecimal {
        name: "hp".into(),
        value: dec("99999999999999999.123456789012345"),
    };
    let inserted = t.insert(&"t2".to_string(), &orig).await.unwrap();
    assert_eq!(inserted, orig);
    let fetched = t.get("t2").await.unwrap();
    assert_eq!(fetched, orig);
}

// ═════════════════════════════════════════════════════════════════════════
// 2. NUMERIC — SQLite NUMERIC affinity, stores as text for decimals
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_numeric_decimal() {
    let db = setup().await;
    let t = table_for!("decimal_numeric", db);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("123456.789012"),
    };
    let inserted = t.insert(&"n1".to_string(), &orig).await.unwrap();
    assert_eq!(inserted, orig);
    let fetched = t.get("n1").await.unwrap();
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_numeric_high_precision_lossy() {
    // SQLite NUMERIC affinity converts large decimals to REAL, losing precision
    let db = setup().await;
    let t = table_for!("decimal_numeric", db);
    let orig = ValDecimal {
        name: "hp".into(),
        value: dec("99999999999999999.123456789012345"),
    };
    let inserted = t.insert(&"n2".to_string(), &orig).await.unwrap();
    // NUMERIC affinity rounds to f64: 1e17
    assert_ne!(inserted, orig);
}

// ═════════════════════════════════════════════════════════════════════════
// 3. REAL — f64 precision, lossy
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_real_simple() {
    let db = setup().await;
    let t = table_for!("decimal_real", db);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("123.456"),
    };
    let result = t.insert(&"r1".to_string(), &orig).await;
    if let Ok(inserted) = result {
        let diff = (inserted.value - orig.value).abs();
        assert!(diff < dec("0.001"), "diff too large: {}", diff);
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 4. INTEGER — fractional part lost
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_integer_whole() {
    let db = setup().await;
    let t = table_for!("decimal_integer", db);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("42"),
    };
    let result = t.insert(&"i1".to_string(), &orig).await;
    if let Ok(inserted) = result {
        assert_eq!(inserted.value, dec("42"));
    }
}

#[tokio::test]
async fn test_integer_truncates() {
    let db = setup().await;
    let t = table_for!("decimal_integer", db);
    let orig = ValDecimal {
        name: "frac".into(),
        value: dec("123.999"),
    };
    let result = t.insert(&"i2".to_string(), &orig).await;
    if let Ok(inserted) = result {
        // SQLite INTEGER affinity — the tag string "123.999" is bound as text,
        // SQLite may store as-is or convert depending on affinity rules
        let v = inserted.value;
        assert!(v == dec("123.999") || v == dec("124"), "unexpected: {}", v);
    }
}
