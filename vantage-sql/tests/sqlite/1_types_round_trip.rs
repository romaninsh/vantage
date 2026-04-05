//! Test 1a: SqliteType system — AnySqliteType in-memory round-trips.
//! Pure type system tests, no database, no Records.

use vantage_sql::sqlite::AnySqliteType;
use vantage_sql::sqlite::types::SqliteTypeVariants;

// ── Round-trips per type ───────────────────────────────────────────────────

#[test]
fn test_integer_round_trip() {
    let val = AnySqliteType::new(42i64);
    assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Integer));
    assert_eq!(val.try_get::<i64>(), Some(42));
}

#[test]
fn test_text_round_trip() {
    let val = AnySqliteType::new("hello".to_string());
    assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Text));
    assert_eq!(val.try_get::<String>(), Some("hello".to_string()));
}

#[test]
fn test_real_round_trip() {
    let val = AnySqliteType::new(3.15f64);
    assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Real));
    assert_eq!(val.try_get::<f64>(), Some(3.15));
}

#[test]
fn test_bool_round_trip() {
    let val = AnySqliteType::new(true);
    assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Bool));
    assert_eq!(val.try_get::<bool>(), Some(true));
    // Bool is distinct from Integer — type boundary enforced
    assert_eq!(val.try_get::<i64>(), None);
}

#[test]
fn test_null_round_trip() {
    let val = AnySqliteType::new(None::<i64>);
    assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Integer));
    assert_eq!(val.try_get::<Option<i64>>(), Some(None));
}

#[test]
fn test_smaller_integers() {
    let val = AnySqliteType::new(127i8);
    assert_eq!(val.try_get::<i8>(), Some(127));
    assert_eq!(val.try_get::<i64>(), Some(127));

    let val = AnySqliteType::new(1000i16);
    assert_eq!(val.try_get::<i16>(), Some(1000));

    let val = AnySqliteType::new(200u8);
    assert_eq!(val.try_get::<u8>(), Some(200));
}

// ── Type mismatch: wrong variant → None ────────────────────────────────────

#[test]
fn test_mismatch_text_as_integer() {
    let val = AnySqliteType::new("hello".to_string());
    assert_eq!(val.try_get::<i64>(), None);
}

#[test]
fn test_mismatch_integer_as_text() {
    let val = AnySqliteType::new(42i64);
    assert_eq!(val.try_get::<String>(), None);
}

#[test]
fn test_mismatch_real_as_integer() {
    let val = AnySqliteType::new(3.15f64);
    assert_eq!(val.try_get::<i64>(), None);
}

#[test]
fn test_mismatch_integer_as_real() {
    let val = AnySqliteType::new(42i64);
    assert_eq!(val.try_get::<f64>(), None);
}

// ── From conversions ───────────────────────────────────────────────────────

#[test]
fn test_from_str() {
    let val: AnySqliteType = "world".into();
    assert_eq!(val.try_get::<String>(), Some("world".to_string()));
}

#[test]
fn test_from_i64() {
    let val: AnySqliteType = 99i64.into();
    assert_eq!(val.try_get::<i64>(), Some(99));
}

#[test]
fn test_from_bool() {
    let val: AnySqliteType = false.into();
    assert_eq!(val.try_get::<bool>(), Some(false));
}
