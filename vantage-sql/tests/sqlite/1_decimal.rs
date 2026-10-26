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
async fn test_text_high_precision() {
    let db = setup().await;
    let t = table_for!("decimal_text", db);
    let orig = ValDecimal {
        name: "hp".into(),
        value: dec("99999999999999999.123456789012345"),
    };
    let inserted = t.insert("t2", &orig).await.unwrap();
    assert_eq!(inserted, orig);
    let fetched = t.get("t2").await.unwrap().expect("row exists");
    assert_eq!(fetched, orig);
}

// ═════════════════════════════════════════════════════════════════════════
// 2. NUMERIC — SQLite NUMERIC affinity, stores as text for decimals
// ═════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_numeric_decimal() {
    // NUMERIC affinity stores this as REAL; it reads back through the f64 arm
    let db = setup().await;
    let t = table_for!("decimal_numeric", db);
    let orig = ValDecimal {
        name: "d".into(),
        value: dec("123456.789012"),
    };
    let inserted = t.insert("n1", &orig).await.unwrap();
    assert_eq!(inserted, orig);
    let fetched = t.get("n1").await.unwrap().expect("row exists");
    assert_eq!(fetched, orig);
}

#[tokio::test]
async fn test_numeric_high_precision_lossy() {
    // SQLite NUMERIC affinity converts large decimals to REAL, losing precision;
    // the integral result (1e17) is then stored as INTEGER
    let db = setup().await;
    let t = table_for!("decimal_numeric", db);
    let orig = ValDecimal {
        name: "hp".into(),
        value: dec("99999999999999999.123456789012345"),
    };
    let inserted = t.insert("n2", &orig).await.unwrap();
    assert_eq!(inserted.value, dec("100000000000000000"));
}
