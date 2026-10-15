//! Test 1b: Record<AnySqliteType> conversions — typed (write path) and
//! untyped (read path) round-trips, error cases.
//!
//! Uses native CborValue for untyped values (simulating database reads)
//! instead of the JSON bridge.

use ciborium::Value as CborValue;
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
        AnySqliteType::untyped(CborValue::Text("Cupcake".into())),
    );
    record.insert(
        "price".into(),
        AnySqliteType::untyped(CborValue::Integer(120.into())),
    );
    record.insert(
        "active".into(),
        AnySqliteType::untyped(CborValue::Bool(true)),
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
    let mut record: Record<AnySqliteType> = Record::new();
    record.insert("note".into(), AnySqliteType::untyped(CborValue::Null));

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

    // Untyped: same CBOR value but no variant — permissive
    let untyped = AnySqliteType::untyped(CborValue::Integer(42.into()));
    assert_eq!(untyped.try_get::<i64>(), Some(42));
}

#[test]
fn test_untyped_integer_narrowing() {
    let val = AnySqliteType::untyped(CborValue::Integer(42.into()));
    assert_eq!(val.try_get::<i64>(), Some(42));
    assert_eq!(val.try_get::<i32>(), Some(42));
    assert_eq!(val.try_get::<i16>(), Some(42));
}

// ── CBOR tag preservation ─────────────────────────────────────────────────

#[test]
fn test_numeric_tag_preserved() {
    let val = AnySqliteType::untyped(CborValue::Tag(
        10,
        Box::new(CborValue::Text("123.456".into())),
    ));
    assert_eq!(
        *val.value(),
        CborValue::Tag(10, Box::new(CborValue::Text("123.456".into())))
    );
}

// ── Bool stored as integer ────────────────────────────────────────────────

#[test]
fn test_typed_bool_in_record() {
    let mut record: Record<AnySqliteType> = Record::new();
    record.insert("active".into(), AnySqliteType::new(true));

    // SQLite stores bool as Integer 0/1
    assert_eq!(*record["active"].value(), CborValue::Integer(1.into()));
    assert_eq!(record["active"].try_get::<bool>(), Some(true));
    // Bool ≠ Integer → blocked
    assert_eq!(record["active"].try_get::<i64>(), None);
}

// ── JSON bridge round-trip ────────────────────────────────────────────────

#[test]
fn test_json_round_trip_integer() {
    let original = AnySqliteType::new(42i64);
    let json: serde_json::Value = original.clone().into();
    assert_eq!(json, serde_json::json!(42));

    let restored = AnySqliteType::from(json);
    assert_eq!(restored.try_get::<i64>(), Some(42));
}

#[test]
fn test_json_round_trip_string() {
    let original = AnySqliteType::new("hello".to_string());
    let json: serde_json::Value = original.into();
    assert_eq!(json, serde_json::json!("hello"));

    let restored = AnySqliteType::from(json);
    assert_eq!(restored.try_get::<String>(), Some("hello".to_string()));
}

#[test]
fn test_json_round_trip_bool() {
    let original = AnySqliteType::new(true);
    let json: serde_json::Value = original.into();
    // SQLite bool is stored as integer 1 in CBOR, so JSON bridge produces number
    assert_eq!(json, serde_json::json!(1));

    // Roundtrip: JSON integer 1 → CBOR Integer(1) — variant is Integer, not Bool.
    // Direct bool try_get won't work (variant mismatch), but i64 does.
    let restored = AnySqliteType::from(json);
    assert_eq!(restored.try_get::<i64>(), Some(1));
}

#[test]
fn test_json_bridge_keeps_non_finite_floats_as_strings() {
    // JSON has no NaN or Infinity; the bridge keeps the value as a string.
    for float in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let json: serde_json::Value = AnySqliteType::untyped(CborValue::Float(float)).into();
        assert!(json.is_string(), "{float} became {json}");
    }
}

#[test]
fn test_json_bridge_keeps_entries_with_non_text_map_keys() {
    let map = CborValue::Map(vec![(
        CborValue::Integer(1.into()),
        CborValue::Text("val".into()),
    )]);
    let json: serde_json::Value = AnySqliteType::untyped(map).into();

    let object = json.as_object().expect("a map becomes a JSON object");
    assert_eq!(object.len(), 1);
    assert_eq!(object.values().next(), Some(&serde_json::json!("val")));
}

#[test]
fn test_json_bridge_renders_bytes_as_text_or_hex() {
    let text: serde_json::Value =
        AnySqliteType::untyped(CborValue::Bytes(b"hello".to_vec())).into();
    assert_eq!(text, serde_json::json!("hello"));

    let hex: serde_json::Value = AnySqliteType::untyped(CborValue::Bytes(vec![0xFF, 0xFE])).into();
    assert_eq!(
        hex.as_str().map(str::to_lowercase),
        Some("fffe".to_string())
    );
}

// ── Error cases ────────────────────────────────────────────────────────────

#[test]
fn test_missing_field_in_record() {
    let record: Record<AnySqliteType> = Record::new();
    assert!(record.get("name").is_none());
}

#[test]
fn test_typed_wrong_extraction() {
    let mut record: Record<AnySqliteType> = Record::new();
    record.insert("name".into(), AnySqliteType::new(42i64));

    // Trying to get String from an Integer-tagged value fails
    assert_eq!(record["name"].try_get::<String>(), None);
}
