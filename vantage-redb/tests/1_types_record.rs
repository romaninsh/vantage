//! Test 1b: Record<AnyRedbType> conversions — typed (write path) and
//! untyped (read path) round-trips, error cases.

use ciborium::Value as CborValue;
use vantage_redb::AnyRedbType;
use vantage_types::Record;

// ── Typed records (write path) ─────────────────────────────────────────────
// Values created with AnyRedbType::new() carry variant tags.

#[test]
fn test_typed_record_creation() {
    let mut record: Record<AnyRedbType> = Record::new();
    record.insert("name".into(), AnyRedbType::new("Cupcake".to_string()));
    record.insert("price".into(), AnyRedbType::new(120i64));
    record.insert("is_deleted".into(), AnyRedbType::new(false));

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
    // Option<T> carries through as the inner T's variant when Some, or as
    // a Null-bearing untyped value when None.
    let mut record: Record<AnyRedbType> = Record::new();
    record.insert("nickname".into(), AnyRedbType::new("Ali".to_string()));
    record.insert("count".into(), AnyRedbType::new(42i64));

    assert_eq!(
        record["nickname"].try_get::<String>(),
        Some("Ali".to_string())
    );
    assert_eq!(record["count"].try_get::<i64>(), Some(42));
}

#[test]
fn test_typed_int_vs_string_blocked() {
    let int_val = AnyRedbType::new(42i64);
    let str_val = AnyRedbType::new("42".to_string());

    // Variant boundary blocks cross-type conversion even when JSON could allow it.
    assert_eq!(int_val.try_get::<String>(), None);
    assert_eq!(str_val.try_get::<i64>(), None);
}

#[test]
fn test_typed_int_vs_float_blocked() {
    let int_val = AnyRedbType::new(42i64);
    assert_eq!(int_val.try_get::<i64>(), Some(42));
    // Int ≠ Float — blocked by variant
    assert_eq!(int_val.try_get::<f64>(), None);

    let float_val = AnyRedbType::new(2.5f64);
    assert_eq!(float_val.try_get::<f64>(), Some(2.5));
    assert_eq!(float_val.try_get::<i64>(), None);
}

// ── Untyped records (read path) ────────────────────────────────────────────
// Values created with AnyRedbType::untyped() have type_variant: None.
// try_get is permissive — it just attempts the conversion.

#[test]
fn test_untyped_record_creation() {
    let mut record: Record<AnyRedbType> = Record::new();
    record.insert(
        "name".into(),
        AnyRedbType::untyped(CborValue::Text("Cupcake".into())),
    );
    record.insert(
        "price".into(),
        AnyRedbType::untyped(CborValue::Integer(120i64.into())),
    );
    record.insert("active".into(), AnyRedbType::untyped(CborValue::Bool(true)));

    assert_eq!(
        record["name"].try_get::<String>(),
        Some("Cupcake".to_string())
    );
    assert_eq!(record["price"].try_get::<i64>(), Some(120));
    assert_eq!(record["active"].try_get::<bool>(), Some(true));

    // Underlying value still can't bridge incompatible CBOR shapes.
    assert_eq!(record["name"].try_get::<i64>(), None);
    assert_eq!(record["price"].try_get::<String>(), None);
}

#[test]
fn test_untyped_null() {
    let mut record: Record<AnyRedbType> = Record::new();
    record.insert("note".into(), AnyRedbType::untyped(CborValue::Null));

    assert_eq!(record["note"].try_get::<Option<String>>(), Some(None));
    assert_eq!(record["note"].try_get::<Option<i64>>(), Some(None));
}

// ── Typed vs untyped comparison ────────────────────────────────────────────

#[test]
fn test_typed_blocks_cross_variant() {
    let typed = AnyRedbType::new(42i64);
    assert_eq!(typed.try_get::<i64>(), Some(42));
    assert_eq!(typed.try_get::<f64>(), None); // Int ≠ Float → blocked
    assert_eq!(typed.try_get::<String>(), None);

    let untyped = AnyRedbType::untyped(CborValue::Integer(42i64.into()));
    assert_eq!(untyped.try_get::<i64>(), Some(42));
    // Untyped: the int CBOR can't decode to f32/f64 because RedbType for
    // floats only accepts CborValue::Float — the boundary is the value
    // shape, not the variant tag.
    assert_eq!(untyped.try_get::<f64>(), None);
    assert_eq!(untyped.try_get::<String>(), None);
}

// ── Bytes type ─────────────────────────────────────────────────────────────

#[test]
fn test_typed_bytes_in_record() {
    let mut record: Record<AnyRedbType> = Record::new();
    record.insert("data".into(), AnyRedbType::new(vec![1u8, 2, 3, 4]));

    assert_eq!(record["data"].try_get::<Vec<u8>>(), Some(vec![1, 2, 3, 4]));
    assert_eq!(record["data"].try_get::<String>(), None);
}

// ── Error cases ────────────────────────────────────────────────────────────

#[test]
fn test_missing_field_in_record() {
    let record: Record<AnyRedbType> = Record::new();
    assert!(record.get("name").is_none());
}

#[test]
fn test_typed_wrong_extraction() {
    let mut record: Record<AnyRedbType> = Record::new();
    record.insert("name".into(), AnyRedbType::new(42i64));
    assert_eq!(record["name"].try_get::<String>(), None);
}

// ── TryFrom<AnyRedbType> for Record<AnyRedbType> ──────────────────────────

#[test]
fn test_try_from_map() {
    let map = CborValue::Map(vec![
        (
            CborValue::Text("name".into()),
            CborValue::Text("Cupcake".into()),
        ),
        (
            CborValue::Text("price".into()),
            CborValue::Integer(120i64.into()),
        ),
        (CborValue::Text("active".into()), CborValue::Bool(true)),
    ]);
    let any = AnyRedbType::untyped(map);
    let record: Record<AnyRedbType> = any.try_into().unwrap();

    assert_eq!(record["name"].try_get::<String>(), Some("Cupcake".into()));
    assert_eq!(record["price"].try_get::<i64>(), Some(120));
    assert_eq!(record["active"].try_get::<bool>(), Some(true));
}

#[test]
fn test_try_from_non_map_fails() {
    let any = AnyRedbType::untyped(CborValue::Text("not a map".into()));
    let result: Result<Record<AnyRedbType>, _> = any.try_into();
    assert!(result.is_err());
}
