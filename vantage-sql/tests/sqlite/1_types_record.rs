//! Test 1b: Record<AnySqliteType> conversions — typed (write path) and
//! untyped (read path) round-trips, error cases.

use vantage_sql::sqlite::AnySqliteType;
use vantage_types::Record;

// ── Typed records (write path) ─────────────────────────────────────────────
// Values created with AnySqliteType::new() carry variant tags.

#[test]
fn test_typed_record_creation() {
    let mut record: Record<AnySqliteType> = Record::new();
    record.insert("name".into(), AnySqliteType::new("Cupcake".to_string()));
    record.insert("price".into(), AnySqliteType::new(120i64));
    record.insert("is_deleted".into(), AnySqliteType::new(false));

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
    let mut record: Record<AnySqliteType> = Record::new();
    record.insert(
        "nickname".into(),
        AnySqliteType::new(Some("Ali".to_string())),
    );
    record.insert("note".into(), AnySqliteType::new(None::<String>));

    assert_eq!(
        record["nickname"].try_get::<Option<String>>(),
        Some(Some("Ali".to_string()))
    );
    assert_eq!(record["note"].try_get::<Option<String>>(), Some(None));
}

// ── Untyped records (read path) ────────────────────────────────────────────
// Values created with AnySqliteType::untyped() have type_variant: None.
// try_get is permissive — it attempts conversion without variant check.

#[test]
fn test_untyped_record_creation() {
    let mut record: Record<AnySqliteType> = Record::new();
    record.insert(
        "name".into(),
        AnySqliteType::untyped(serde_json::json!("Cupcake")),
    );
    record.insert(
        "price".into(),
        AnySqliteType::untyped(serde_json::json!(120)),
    );
    record.insert(
        "active".into(),
        AnySqliteType::untyped(serde_json::json!(true)),
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
    let mut record: Record<AnySqliteType> = Record::new();
    record.insert(
        "note".into(),
        AnySqliteType::untyped(serde_json::json!(null)),
    );

    // Untyped null → Option<String> works (no variant blocking it)
    assert_eq!(record["note"].try_get::<Option<String>>(), Some(None));
    assert_eq!(record["note"].try_get::<Option<i64>>(), Some(None));
}

// ── Typed vs untyped comparison ────────────────────────────────────────────

#[test]
fn test_typed_blocks_cross_variant() {
    // Typed: integer value with Integer variant
    let typed = AnySqliteType::new(42i64);
    assert_eq!(typed.try_get::<i64>(), Some(42));
    assert_eq!(typed.try_get::<f64>(), None); // Integer ≠ Real → blocked

    // Untyped: same JSON value but no variant
    let untyped = AnySqliteType::untyped(serde_json::json!(42));
    assert_eq!(untyped.try_get::<i64>(), Some(42));
    // Still None — not because of variant, but because json 42 doesn't parse as f64
    // (serde_json::Number::as_f64 works on integers though, so this actually succeeds
    // for untyped... let's verify)
}

#[test]
fn test_untyped_is_permissive_across_numeric() {
    // Untyped integer: try_get as f64 works because json Number can be read as f64
    let untyped = AnySqliteType::untyped(serde_json::json!(42));
    assert_eq!(untyped.try_get::<i64>(), Some(42));
    // f64::from_json checks Value::Number → as_f64() which works for integer numbers
    assert_eq!(untyped.try_get::<f64>(), Some(42.0));

    // Typed integer: f64 blocked by variant
    let typed = AnySqliteType::new(42i64);
    assert_eq!(typed.try_get::<f64>(), None); // Integer ≠ Real
}

// ── Error cases ────────────────────────────────────────────────────────────

#[test]
fn test_missing_field_in_record() {
    let record: Record<AnySqliteType> = Record::new();
    // Empty record — accessing a missing field
    assert!(record.get("name").is_none());
}

#[test]
fn test_typed_wrong_extraction() {
    let mut record: Record<AnySqliteType> = Record::new();
    record.insert("name".into(), AnySqliteType::new(42i64)); // stored as Integer

    // Trying to get String from an Integer-tagged value fails
    assert_eq!(record["name"].try_get::<String>(), None);
}
