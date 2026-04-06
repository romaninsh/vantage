//! Test 1a: MysqlType system — AnyMysqlType in-memory round-trips.
//! Pure type system tests, no database, no Records.

use vantage_sql::mysql::AnyMysqlType;
use vantage_sql::mysql::types::MysqlTypeVariants;

// ── Round-trips per type ───────────────────────────────────────────────────

#[test]
fn test_integer_round_trip() {
    let val = AnyMysqlType::new(42i64);
    assert_eq!(val.type_variant(), Some(MysqlTypeVariants::Int8));
    assert_eq!(val.try_get::<i64>(), Some(42));
}

#[test]
fn test_i32_round_trip() {
    let val = AnyMysqlType::new(42i32);
    assert_eq!(val.type_variant(), Some(MysqlTypeVariants::Int4));
    assert_eq!(val.try_get::<i32>(), Some(42));
}

#[test]
fn test_i16_round_trip() {
    let val = AnyMysqlType::new(42i16);
    assert_eq!(val.type_variant(), Some(MysqlTypeVariants::Int2));
    assert_eq!(val.try_get::<i16>(), Some(42));
}

#[test]
fn test_text_round_trip() {
    let val = AnyMysqlType::new("hello".to_string());
    assert_eq!(val.type_variant(), Some(MysqlTypeVariants::Text));
    assert_eq!(val.try_get::<String>(), Some("hello".to_string()));
}

#[test]
fn test_float8_round_trip() {
    let val = AnyMysqlType::new(3.15f64);
    assert_eq!(val.type_variant(), Some(MysqlTypeVariants::Float8));
    assert_eq!(val.try_get::<f64>(), Some(3.15));
}

#[test]
fn test_float4_round_trip() {
    let val = AnyMysqlType::new(3.15f32);
    assert_eq!(val.type_variant(), Some(MysqlTypeVariants::Float4));
    // f32 round-trip goes through f64, so approximate check
    let result = val.try_get::<f32>().unwrap();
    assert!((result - 3.15f32).abs() < 0.001);
}

#[test]
fn test_bool_round_trip() {
    let val = AnyMysqlType::new(true);
    assert_eq!(val.type_variant(), Some(MysqlTypeVariants::Bool));
    assert_eq!(val.try_get::<bool>(), Some(true));
    // Bool is distinct from Int8 — type boundary enforced
    assert_eq!(val.try_get::<i64>(), None);
}

#[test]
fn test_null_round_trip() {
    let val = AnyMysqlType::new(None::<i64>);
    assert_eq!(val.type_variant(), Some(MysqlTypeVariants::Int8));
    assert_eq!(val.try_get::<Option<i64>>(), Some(None));
}

#[test]
fn test_smaller_integers() {
    let val = AnyMysqlType::new(127i8);
    assert_eq!(val.try_get::<i8>(), Some(127));
    // i8 maps to Int2 in mysql, but underlying JSON is the same number
    assert_eq!(val.try_get::<i16>(), Some(127));

    let val = AnyMysqlType::new(1000i16);
    assert_eq!(val.try_get::<i16>(), Some(1000));

    let val = AnyMysqlType::new(200u8);
    assert_eq!(val.try_get::<u8>(), Some(200));
}

// ── Type mismatch: wrong variant → None ────────────────────────────────────

#[test]
fn test_mismatch_text_as_integer() {
    let val = AnyMysqlType::new("hello".to_string());
    assert_eq!(val.try_get::<i64>(), None);
}

#[test]
fn test_mismatch_integer_as_text() {
    let val = AnyMysqlType::new(42i64);
    assert_eq!(val.try_get::<String>(), None);
}

#[test]
fn test_mismatch_real_as_integer() {
    let val = AnyMysqlType::new(3.15f64);
    assert_eq!(val.try_get::<i64>(), None);
}

#[test]
fn test_mismatch_integer_as_real() {
    let val = AnyMysqlType::new(42i64);
    assert_eq!(val.try_get::<f64>(), None);
}

// ── From conversions ───────────────────────────────────────────────────────

#[test]
fn test_from_str() {
    let val: AnyMysqlType = "world".into();
    assert_eq!(val.try_get::<String>(), Some("world".to_string()));
}

#[test]
fn test_from_i64() {
    let val: AnyMysqlType = 99i64.into();
    assert_eq!(val.try_get::<i64>(), Some(99));
}

#[test]
fn test_from_bool() {
    let val: AnyMysqlType = false.into();
    assert_eq!(val.try_get::<bool>(), Some(false));
}

// ── MySQL-specific: distinct integer widths ────────────────────────────────

#[test]
fn test_i32_vs_i64_variant_boundary() {
    // i32 maps to Int4, i64 maps to Int8 — they are different variants
    let val32 = AnyMysqlType::new(42i32);
    let val64 = AnyMysqlType::new(42i64);
    assert_eq!(val32.type_variant(), Some(MysqlTypeVariants::Int4));
    assert_eq!(val64.type_variant(), Some(MysqlTypeVariants::Int8));

    // Cross-variant extraction is blocked by type markers
    assert_eq!(val32.try_get::<i64>(), None); // Int4 ≠ Int8
    assert_eq!(val64.try_get::<i32>(), None); // Int8 ≠ Int4
}

#[test]
fn test_f32_vs_f64_variant_boundary() {
    let val32 = AnyMysqlType::new(1.5f32);
    let val64 = AnyMysqlType::new(1.5f64);
    assert_eq!(val32.type_variant(), Some(MysqlTypeVariants::Float4));
    assert_eq!(val64.type_variant(), Some(MysqlTypeVariants::Float8));

    // Cross-variant extraction blocked
    assert_eq!(val32.try_get::<f64>(), None); // Float4 ≠ Float8
    assert_eq!(val64.try_get::<f32>(), None); // Float8 ≠ Float4
}
