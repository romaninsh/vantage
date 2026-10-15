//! Test 1c: variant-preserving CBOR row encoding (`encode_record`/`decode_record`)
//! and index key encoding (`value_to_index_key`). These functions are the
//! storage boundary — every byte that goes to disk passes through them.

use ciborium::Value as CborValue;
use vantage_redb::types::{decode_record, encode_record, encode_value, value_to_index_key};
use vantage_redb::{AnyRedbType, RedbTypeVariants};
use vantage_types::Record;

fn cbor_bytes(value: &CborValue) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes).unwrap();
    bytes
}

// ── Round-trip with full variant tagging ───────────────────────────────────

#[test]
fn test_round_trip_typed_preserves_variants() {
    let mut rec: Record<AnyRedbType> = Record::new();
    rec.insert("name".into(), AnyRedbType::new("Alice".to_string()));
    rec.insert("age".into(), AnyRedbType::new(30i64));
    rec.insert("score".into(), AnyRedbType::new(0.5f64));
    rec.insert("active".into(), AnyRedbType::new(true));
    rec.insert("data".into(), AnyRedbType::new(vec![1u8, 2, 3]));

    let bytes = encode_record(&rec).unwrap();
    let back = decode_record(&bytes).unwrap();

    assert_eq!(back.len(), 5);
    assert_eq!(back["name"].try_get::<String>(), Some("Alice".to_string()));
    assert_eq!(back["age"].try_get::<i64>(), Some(30));
    assert_eq!(back["score"].try_get::<f64>(), Some(0.5));
    assert_eq!(back["active"].try_get::<bool>(), Some(true));
    assert_eq!(back["data"].try_get::<Vec<u8>>(), Some(vec![1, 2, 3]));

    assert_eq!(back["name"].type_variant(), Some(RedbTypeVariants::String));
    assert_eq!(back["age"].type_variant(), Some(RedbTypeVariants::Int));
    assert_eq!(back["score"].type_variant(), Some(RedbTypeVariants::Float));
    assert_eq!(back["active"].type_variant(), Some(RedbTypeVariants::Bool));
    assert_eq!(back["data"].type_variant(), Some(RedbTypeVariants::Bytes));

    assert_eq!(back["age"].try_get::<f64>(), None);
    assert_eq!(back["name"].try_get::<i64>(), None);
}

#[test]
fn test_round_trip_empty_record() {
    let rec: Record<AnyRedbType> = Record::new();

    let back = decode_record(&encode_record(&rec).unwrap()).unwrap();

    assert_eq!(back.len(), 0);
}

#[test]
fn test_round_trip_preserves_field_order() {
    let mut rec: Record<AnyRedbType> = Record::new();
    rec.insert("c".into(), AnyRedbType::new(3i64));
    rec.insert("a".into(), AnyRedbType::new(1i64));
    rec.insert("b".into(), AnyRedbType::new(2i64));

    let back = decode_record(&encode_record(&rec).unwrap()).unwrap();

    let names: Vec<&String> = back.keys().collect();
    assert_eq!(names, vec!["c", "a", "b"]);
}

// ── Untyped on write → re-tagged on read from CBOR shape ──────────────────

#[test]
fn test_untyped_int_retagged_from_cbor() {
    let mut rec: Record<AnyRedbType> = Record::new();
    rec.insert(
        "x".into(),
        AnyRedbType::untyped(CborValue::Integer(7i64.into())),
    );

    let back = decode_record(&encode_record(&rec).unwrap()).unwrap();

    assert_eq!(back["x"].type_variant(), Some(RedbTypeVariants::Int));
    assert_eq!(back["x"].try_get::<i64>(), Some(7));
}

#[test]
fn test_untyped_string_retagged_from_cbor() {
    let mut rec: Record<AnyRedbType> = Record::new();
    rec.insert(
        "s".into(),
        AnyRedbType::untyped(CborValue::Text("hello".into())),
    );

    let back = decode_record(&encode_record(&rec).unwrap()).unwrap();

    assert_eq!(back["s"].type_variant(), Some(RedbTypeVariants::String));
    assert_eq!(back["s"].try_get::<String>(), Some("hello".into()));
}

#[test]
fn test_untyped_bool_retagged_from_cbor() {
    let mut rec: Record<AnyRedbType> = Record::new();
    rec.insert("b".into(), AnyRedbType::untyped(CborValue::Bool(true)));

    let back = decode_record(&encode_record(&rec).unwrap()).unwrap();

    assert_eq!(back["b"].type_variant(), Some(RedbTypeVariants::Bool));
}

#[test]
fn test_untyped_null_stays_null() {
    let mut rec: Record<AnyRedbType> = Record::new();
    rec.insert("n".into(), AnyRedbType::untyped(CborValue::Null));

    let back = decode_record(&encode_record(&rec).unwrap()).unwrap();

    assert_eq!(back["n"].type_variant(), Some(RedbTypeVariants::Null));
}

#[test]
fn test_untyped_float_bytes_array_and_map_retagged_from_cbor() {
    let mut rec: Record<AnyRedbType> = Record::new();
    rec.insert("f".into(), AnyRedbType::untyped(CborValue::Float(1.5)));
    rec.insert(
        "b".into(),
        AnyRedbType::untyped(CborValue::Bytes(vec![1, 2])),
    );
    rec.insert(
        "a".into(),
        AnyRedbType::untyped(CborValue::Array(vec![CborValue::Bool(true)])),
    );
    rec.insert(
        "m".into(),
        AnyRedbType::untyped(CborValue::Map(vec![(
            CborValue::Text("k".into()),
            CborValue::Integer(1.into()),
        )])),
    );

    let back = decode_record(&encode_record(&rec).unwrap()).unwrap();

    assert_eq!(back["f"].type_variant(), Some(RedbTypeVariants::Float));
    assert_eq!(back["b"].type_variant(), Some(RedbTypeVariants::Bytes));
    assert_eq!(back["a"].type_variant(), Some(RedbTypeVariants::Array));
    assert_eq!(back["m"].type_variant(), Some(RedbTypeVariants::Map));
}

#[test]
fn test_variant_from_cbor_looks_through_tags() {
    let tagged = CborValue::Tag(1, Box::new(CborValue::Integer(5.into())));

    assert_eq!(
        RedbTypeVariants::from_cbor(&tagged),
        Some(RedbTypeVariants::Int)
    );
}

#[test]
fn test_variant_index_round_trips_for_every_variant() {
    let all = [
        RedbTypeVariants::Null,
        RedbTypeVariants::Bool,
        RedbTypeVariants::Int,
        RedbTypeVariants::Float,
        RedbTypeVariants::String,
        RedbTypeVariants::Bytes,
        RedbTypeVariants::Array,
        RedbTypeVariants::Map,
    ];
    let mut seen = Vec::new();
    for variant in all {
        let index = variant.to_index();
        assert_eq!(RedbTypeVariants::from_index(index), Some(variant));
        assert!(!seen.contains(&index), "index {index} is used twice");
        seen.push(index);
    }
    assert_eq!(RedbTypeVariants::from_index(200), None);
}

// ── Decode error paths ────────────────────────────────────────────────────

#[test]
fn test_decode_empty_fails() {
    let result = decode_record(&[]);
    assert!(result.is_err());
}

#[test]
fn test_decode_garbage_fails() {
    let result = decode_record(&[0xFF, 0xFE, 0xFD]);
    assert!(result.is_err());
}

#[test]
fn test_decode_valid_cbor_that_is_not_a_row_body_fails() {
    let not_an_array = cbor_bytes(&CborValue::Text("row".into()));
    assert!(decode_record(&not_an_array).is_err());

    let pair_not_triple = cbor_bytes(&CborValue::Array(vec![CborValue::Array(vec![
        CborValue::Text("name".into()),
        CborValue::Null,
    ])]));
    assert!(decode_record(&pair_not_triple).is_err());

    let name_not_text = cbor_bytes(&CborValue::Array(vec![CborValue::Array(vec![
        CborValue::Integer(1.into()),
        CborValue::Null,
        CborValue::Null,
    ])]));
    assert!(decode_record(&name_not_text).is_err());

    let tag_not_integer_or_null = cbor_bytes(&CborValue::Array(vec![CborValue::Array(vec![
        CborValue::Text("name".into()),
        CborValue::Text("int".into()),
        CborValue::Null,
    ])]));
    assert!(decode_record(&tag_not_integer_or_null).is_err());
}

// ── Index key encoding ────────────────────────────────────────────────────

#[test]
fn test_index_key_typed_and_untyped_match() {
    // Lookups by `column.eq(value)` must hit rows written through either path.
    let typed = AnyRedbType::new(42i64);
    let untyped = AnyRedbType::untyped(CborValue::Integer(42i64.into()));
    assert_eq!(
        value_to_index_key(&typed).unwrap(),
        value_to_index_key(&untyped).unwrap()
    );
}

#[test]
fn test_index_keys_differ_by_value() {
    let a = AnyRedbType::new(42i64);
    let b = AnyRedbType::new(43i64);
    assert_ne!(
        value_to_index_key(&a).unwrap(),
        value_to_index_key(&b).unwrap()
    );
}

#[test]
fn test_index_keys_differ_by_type_shape() {
    let int_key = value_to_index_key(&AnyRedbType::new(42i64)).unwrap();
    let str_key = value_to_index_key(&AnyRedbType::new("42".to_string())).unwrap();
    assert_ne!(int_key, str_key);
}

#[test]
fn test_index_key_is_the_encoded_value_without_a_variant_tag() {
    let value = AnyRedbType::new("alice@x.com".to_string());

    assert_eq!(
        value_to_index_key(&value).unwrap(),
        encode_value(value.value()).unwrap()
    );
}
