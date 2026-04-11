//! Test 1b: Record<AnyMysqlType> conversions — typed (write path) and
//! untyped (read path) round-trips, error cases.
//!
//! Uses native CborValue for untyped values (simulating database reads)
//! instead of the JSON bridge.

use ciborium::Value as CborValue;
use vantage_sql::mysql::AnyMysqlType;
use vantage_types::Record;

// ── Typed records (write path) ─────────────────────────────────────────────
// Values created with AnyMysqlType::new() carry variant tags.

#[test]
fn test_typed_record_creation() {
    let mut record: Record<AnyMysqlType> = Record::new();
    record.insert("name".into(), AnyMysqlType::new("Cupcake".to_string()));
    record.insert("price".into(), AnyMysqlType::new(120i64));
    record.insert("is_deleted".into(), AnyMysqlType::new(false));

    assert_eq!(
        record["name"].try_get::<String>(),
        Some("Cupcake".to_string())
    );
    assert_eq!(record["price"].try_get::<i64>(), Some(120));
    assert_eq!(record["is_deleted"].try_get::<bool>(), Some(false));

    // Type markers enforce boundaries — wrong type → None
    assert_eq!(record["name"].try_get::<i64>(), None);
    assert_eq!(record["price"].try_get::<String>(), None);
}

#[test]
fn test_typed_record_with_option() {
    let mut record: Record<AnyMysqlType> = Record::new();
    record.insert(
        "nickname".into(),
        AnyMysqlType::new(Some("Ali".to_string())),
    );
    record.insert("note".into(), AnyMysqlType::new(None::<String>));

    assert_eq!(
        record["nickname"].try_get::<Option<String>>(),
        Some(Some("Ali".to_string()))
    );
    assert_eq!(record["note"].try_get::<Option<String>>(), Some(None));
}

// ── Untyped records (read path) ────────────────────────────────────────────
// Values created with AnyMysqlType::untyped() have type_variant: None.
// try_get is permissive — it attempts conversion without variant check.

#[test]
fn test_untyped_record_creation() {
    let mut record: Record<AnyMysqlType> = Record::new();
    record.insert(
        "name".into(),
        AnyMysqlType::untyped(CborValue::Text("Cupcake".into())),
    );
    record.insert(
        "price".into(),
        AnyMysqlType::untyped(CborValue::Integer(120.into())),
    );
    record.insert(
        "active".into(),
        AnyMysqlType::untyped(CborValue::Bool(true)),
    );

    assert_eq!(
        record["name"].try_get::<String>(),
        Some("Cupcake".to_string())
    );
    assert_eq!(record["price"].try_get::<i64>(), Some(120));
    assert_eq!(record["active"].try_get::<bool>(), Some(true));

    // Still fails when the underlying value can't convert
    assert_eq!(record["name"].try_get::<i64>(), None);
    assert_eq!(record["price"].try_get::<String>(), None);
}

#[test]
fn test_untyped_null() {
    let mut record: Record<AnyMysqlType> = Record::new();
    record.insert("note".into(), AnyMysqlType::untyped(CborValue::Null));

    assert_eq!(record["note"].try_get::<Option<String>>(), Some(None));
    assert_eq!(record["note"].try_get::<Option<i64>>(), Some(None));
}

// ── Typed vs untyped comparison ────────────────────────────────────────────

#[test]
fn test_typed_blocks_cross_variant() {
    // Typed: integer value with Int8 variant
    let typed = AnyMysqlType::new(42i64);
    assert_eq!(typed.try_get::<i64>(), Some(42));
    assert_eq!(typed.try_get::<f64>(), None); // Int8 ≠ Float8 → blocked

    // Untyped: same CBOR value but no variant — permissive
    let untyped = AnyMysqlType::untyped(CborValue::Integer(42.into()));
    assert_eq!(untyped.try_get::<i64>(), Some(42));
}

#[test]
fn test_untyped_integer_narrowing() {
    let val = AnyMysqlType::untyped(CborValue::Integer(42.into()));
    assert_eq!(val.try_get::<i64>(), Some(42));
    assert_eq!(val.try_get::<i32>(), Some(42));
    assert_eq!(val.try_get::<i16>(), Some(42));
}

// ── CBOR tag preservation ─────────────────────────────────────────────────

#[test]
fn test_decimal_tag_preserved() {
    let val = AnyMysqlType::untyped(CborValue::Tag(
        10,
        Box::new(CborValue::Text("123.456".into())),
    ));
    // Decimal tag should be detectable
    assert_eq!(
        *val.value(),
        CborValue::Tag(10, Box::new(CborValue::Text("123.456".into())))
    );
}

#[test]
fn test_datetime_tag_preserved() {
    let val = AnyMysqlType::untyped(CborValue::Tag(
        0,
        Box::new(CborValue::Text("2024-01-15T10:30:00".into())),
    ));
    assert_eq!(
        *val.value(),
        CborValue::Tag(0, Box::new(CborValue::Text("2024-01-15T10:30:00".into())))
    );
}

// ── Bool stored natively ──────────────────────────────────────────────────

#[test]
fn test_typed_bool_in_record() {
    let mut record: Record<AnyMysqlType> = Record::new();
    record.insert("active".into(), AnyMysqlType::new(true));

    assert_eq!(*record["active"].value(), CborValue::Bool(true));
    assert_eq!(record["active"].try_get::<bool>(), Some(true));
    // Bool ≠ Int8 → blocked
    assert_eq!(record["active"].try_get::<i64>(), None);
}

// ── JSON bridge round-trip ────────────────────────────────────────────────

#[test]
fn test_json_round_trip_integer() {
    let original = AnyMysqlType::new(42i64);
    let json: serde_json::Value = original.clone().into();
    assert_eq!(json, serde_json::json!(42));

    let restored = AnyMysqlType::from(json);
    assert_eq!(restored.try_get::<i64>(), Some(42));
}

#[test]
fn test_json_round_trip_string() {
    let original = AnyMysqlType::new("hello".to_string());
    let json: serde_json::Value = original.into();
    assert_eq!(json, serde_json::json!("hello"));

    let restored = AnyMysqlType::from(json);
    assert_eq!(restored.try_get::<String>(), Some("hello".to_string()));
}

#[test]
fn test_json_round_trip_bool() {
    let original = AnyMysqlType::new(true);
    let json: serde_json::Value = original.into();
    assert_eq!(json, serde_json::json!(true));

    let restored = AnyMysqlType::from(json);
    assert_eq!(restored.try_get::<bool>(), Some(true));
}

// ── Error cases ────────────────────────────────────────────────────────────

#[test]
fn test_missing_field_in_record() {
    let record: Record<AnyMysqlType> = Record::new();
    assert!(record.get("name").is_none());
}

#[test]
fn test_typed_wrong_extraction() {
    let mut record: Record<AnyMysqlType> = Record::new();
    record.insert("name".into(), AnyMysqlType::new(42i64));

    // Trying to get String from an Int8-tagged value fails
    assert_eq!(record["name"].try_get::<String>(), None);
}
