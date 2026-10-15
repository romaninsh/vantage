//! Test 1b: Record<AnyRedbType> conversions — typed (write path) and
//! untyped (read path) round-trips, error cases.

use ciborium::Value as CborValue;
use vantage_redb::{AnyRedbType, RedbTypeVariants};
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
fn test_typed_variants_follow_the_rust_type() {
    assert_eq!(
        AnyRedbType::new(42i64).type_variant(),
        Some(RedbTypeVariants::Int)
    );
    assert_eq!(
        AnyRedbType::new("hello".to_string()).type_variant(),
        Some(RedbTypeVariants::String)
    );
    assert_eq!(
        AnyRedbType::new(true).type_variant(),
        Some(RedbTypeVariants::Bool)
    );
    assert_eq!(
        AnyRedbType::new(2.5f64).type_variant(),
        Some(RedbTypeVariants::Float)
    );
    assert_eq!(
        AnyRedbType::new(vec![1u8]).type_variant(),
        Some(RedbTypeVariants::Bytes)
    );
}

#[test]
fn test_typed_record_with_option() {
    let some = AnyRedbType::new(Some(42i64));
    assert_eq!(some.type_variant(), Some(RedbTypeVariants::Int));
    assert_eq!(some.try_get::<i64>(), Some(42));
    assert_eq!(some.try_get::<Option<i64>>(), Some(Some(42)));

    let none = AnyRedbType::new(None::<i64>);
    assert_eq!(none.value(), &CborValue::Null);
    assert_eq!(none.try_get::<Option<i64>>(), Some(None));
}

#[test]
fn test_typed_int_vs_string_blocked() {
    let int_val = AnyRedbType::new(42i64);
    let str_val = AnyRedbType::new("42".to_string());

    assert_eq!(int_val.try_get::<String>(), None);
    assert_eq!(str_val.try_get::<i64>(), None);
}

#[test]
fn test_typed_int_vs_float_blocked() {
    let int_val = AnyRedbType::new(42i64);
    assert_eq!(int_val.try_get::<i64>(), Some(42));
    assert_eq!(int_val.try_get::<f64>(), None);

    let float_val = AnyRedbType::new(2.5f64);
    assert_eq!(float_val.try_get::<f64>(), Some(2.5));
    assert_eq!(float_val.try_get::<i64>(), None);
}

#[test]
fn test_float_f32_round_trips() {
    let v = AnyRedbType::new(1.5f32);

    assert_eq!(v.type_variant(), Some(RedbTypeVariants::Float));
    assert_eq!(v.try_get::<f32>(), Some(1.5));
}

#[test]
fn test_integer_widths_share_the_int_variant_and_respect_their_range() {
    assert_eq!(
        AnyRedbType::new(7u8).type_variant(),
        Some(RedbTypeVariants::Int)
    );
    assert_eq!(AnyRedbType::new(300i64).try_get::<u8>(), None);
    assert_eq!(AnyRedbType::new(200i64).try_get::<u8>(), Some(200));
    assert_eq!(AnyRedbType::new(-1i64).try_get::<u64>(), None);
}

#[test]
fn test_typed_wrong_extraction() {
    let mut record: Record<AnyRedbType> = Record::new();
    record.insert("name".into(), AnyRedbType::new(42i64));
    assert_eq!(record["name"].try_get::<String>(), None);
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
fn test_untyped_has_no_variant_and_is_permissive() {
    let v = AnyRedbType::untyped(CborValue::Integer(42i64.into()));

    assert_eq!(v.type_variant(), None);
    assert_eq!(v.try_get::<i64>(), Some(42));
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
    assert_eq!(typed.try_get::<f64>(), None);
    assert_eq!(typed.try_get::<String>(), None);

    let untyped = AnyRedbType::untyped(CborValue::Integer(42i64.into()));
    assert_eq!(untyped.try_get::<i64>(), Some(42));
    // A CBOR integer is not a CBOR float, so even the permissive path refuses.
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

// ── Conversions ────────────────────────────────────────────────────────────

#[test]
fn test_try_from_extracts_scalars_and_errors_on_a_mismatch() {
    assert_eq!(i64::try_from(AnyRedbType::new(5i64)).unwrap(), 5);
    assert_eq!(
        String::try_from(AnyRedbType::new("x".to_string())).unwrap(),
        "x"
    );
    assert!(bool::try_from(AnyRedbType::new(true)).unwrap());
    assert_eq!(f64::try_from(AnyRedbType::new(0.25f64)).unwrap(), 0.25);
    assert_eq!(
        Vec::<u8>::try_from(AnyRedbType::new(vec![9u8])).unwrap(),
        vec![9]
    );

    assert!(i64::try_from(AnyRedbType::new("x".to_string())).is_err());
    assert!(Vec::<u8>::try_from(AnyRedbType::new(1i64)).is_err());
}

#[test]
fn test_from_native_values_builds_typed_values() {
    assert_eq!(
        AnyRedbType::from("hi").type_variant(),
        Some(RedbTypeVariants::String)
    );
    assert_eq!(
        AnyRedbType::from(3i32).type_variant(),
        Some(RedbTypeVariants::Int)
    );
    assert_eq!(
        AnyRedbType::from(true).type_variant(),
        Some(RedbTypeVariants::Bool)
    );
    assert_eq!(
        AnyRedbType::from("hi").try_get::<String>().as_deref(),
        Some("hi")
    );
}

#[test]
fn test_display_shows_each_kind_of_value() {
    assert_eq!(AnyRedbType::untyped(CborValue::Null).to_string(), "null");
    assert_eq!(AnyRedbType::new(true).to_string(), "true");
    assert_eq!(AnyRedbType::new(2.5f64).to_string(), "2.5");
    assert_eq!(AnyRedbType::new("hi".to_string()).to_string(), "\"hi\"");
    assert_eq!(
        AnyRedbType::new(vec![1u8, 2, 3]).to_string(),
        "Bytes(3 bytes)"
    );
}

#[test]
#[ignore = "possible bug: Display of an integer prints the CBOR debug form `Integer(42)`, not `42` like bool and float"]
fn test_display_shows_an_integer_as_its_digits() {
    assert_eq!(AnyRedbType::new(42i64).to_string(), "42");
}

// ── Error cases ────────────────────────────────────────────────────────────

#[test]
fn test_missing_field_in_record() {
    let record: Record<AnyRedbType> = Record::new();
    assert!(record.get("name").is_none());
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
