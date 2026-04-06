//! Test 1b: Record<AnyMysqlType> conversions — typed (write path) and
//! untyped (read path) round-trips, error cases.

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

    // Values carry their type markers
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
        AnyMysqlType::untyped(serde_json::json!("Cupcake")),
    );
    record.insert(
        "price".into(),
        AnyMysqlType::untyped(serde_json::json!(120)),
    );
    record.insert(
        "active".into(),
        AnyMysqlType::untyped(serde_json::json!(true)),
    );

    // Untyped values — try_get just attempts the conversion
    assert_eq!(
        record["name"].try_get::<String>(),
        Some("Cupcake".to_string())
    );
    assert_eq!(record["price"].try_get::<i64>(), Some(120));
    assert_eq!(record["active"].try_get::<bool>(), Some(true));

    // Still fails when the underlying value can't convert
    assert_eq!(record["name"].try_get::<i64>(), None); // "Cupcake" isn't a number
    assert_eq!(record["price"].try_get::<String>(), None); // 120 isn't a string
}

#[test]
fn test_untyped_null() {
    let mut record: Record<AnyMysqlType> = Record::new();
    record.insert(
        "note".into(),
        AnyMysqlType::untyped(serde_json::json!(null)),
    );

    // Untyped null → Option<String> works (no variant blocking it)
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

    // Untyped: same JSON value but no variant
    let untyped = AnyMysqlType::untyped(serde_json::json!(42));
    assert_eq!(untyped.try_get::<i64>(), Some(42));
}

#[test]
fn test_untyped_is_permissive_across_numeric() {
    // Untyped integer: try_get as f64 works because json Number can be read as f64
    let untyped = AnyMysqlType::untyped(serde_json::json!(42));
    assert_eq!(untyped.try_get::<i64>(), Some(42));
    assert_eq!(untyped.try_get::<f64>(), Some(42.0));

    // Typed integer: f64 blocked by variant
    let typed = AnyMysqlType::new(42i64);
    assert_eq!(typed.try_get::<f64>(), None); // Int8 ≠ Float8
}

// ── MySQL-specific: bool stored natively ──────────────────────────────────

#[test]
fn test_typed_bool_in_record() {
    let mut record: Record<AnyMysqlType> = Record::new();
    record.insert("active".into(), AnyMysqlType::new(true));

    // MySQL stores bool as native JSON bool
    assert_eq!(*record["active"].value(), serde_json::json!(true));
    assert_eq!(record["active"].try_get::<bool>(), Some(true));
    // Bool ≠ Int8 → blocked
    assert_eq!(record["active"].try_get::<i64>(), None);
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
