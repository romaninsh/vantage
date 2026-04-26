//! Test 1c: variant-preserving CBOR row encoding (`encode_record`/`decode_record`)
//! and index key encoding (`value_to_index_key`). These functions are the
//! storage boundary — every byte that goes to disk passes through them.

use ciborium::Value as CborValue;
use vantage_redb::types::{decode_record, encode_record, value_to_index_key};
use vantage_redb::{AnyRedbType, RedbTypeVariants};
use vantage_types::Record;

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

    // Variant tags survive the round-trip — try_get's variant boundary still
    // blocks cross-type extraction.
    assert_eq!(back["name"].type_variant(), Some(RedbTypeVariants::String));
    assert_eq!(back["age"].type_variant(), Some(RedbTypeVariants::Int));
    assert_eq!(back["score"].type_variant(), Some(RedbTypeVariants::Float));
    assert_eq!(back["active"].type_variant(), Some(RedbTypeVariants::Bool));
    assert_eq!(back["data"].type_variant(), Some(RedbTypeVariants::Bytes));

    // Boundary still enforced after round-trip.
    assert_eq!(back["age"].try_get::<f64>(), None);
    assert_eq!(back["name"].try_get::<i64>(), None);
}

#[test]
fn test_round_trip_empty_record() {
    let rec: Record<AnyRedbType> = Record::new();
    let bytes = encode_record(&rec).unwrap();
    let back = decode_record(&bytes).unwrap();
    assert_eq!(back.len(), 0);
}

#[test]
fn test_round_trip_preserves_field_order() {
    let mut rec: Record<AnyRedbType> = Record::new();
    rec.insert("c".into(), AnyRedbType::new(3i64));
    rec.insert("a".into(), AnyRedbType::new(1i64));
    rec.insert("b".into(), AnyRedbType::new(2i64));

    let bytes = encode_record(&rec).unwrap();
    let back = decode_record(&bytes).unwrap();

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

    let bytes = encode_record(&rec).unwrap();
    let back = decode_record(&bytes).unwrap();

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

    let bytes = encode_record(&rec).unwrap();
    let back = decode_record(&bytes).unwrap();

    assert_eq!(back["s"].type_variant(), Some(RedbTypeVariants::String));
    assert_eq!(back["s"].try_get::<String>(), Some("hello".into()));
}

#[test]
fn test_untyped_bool_retagged_from_cbor() {
    let mut rec: Record<AnyRedbType> = Record::new();
    rec.insert("b".into(), AnyRedbType::untyped(CborValue::Bool(true)));

    let bytes = encode_record(&rec).unwrap();
    let back = decode_record(&bytes).unwrap();

    assert_eq!(back["b"].type_variant(), Some(RedbTypeVariants::Bool));
}

#[test]
fn test_untyped_null_stays_null() {
    let mut rec: Record<AnyRedbType> = Record::new();
    rec.insert("n".into(), AnyRedbType::untyped(CborValue::Null));

    let bytes = encode_record(&rec).unwrap();
    let back = decode_record(&bytes).unwrap();

    assert_eq!(back["n"].type_variant(), Some(RedbTypeVariants::Null));
}

// ── Decode error paths ────────────────────────────────────────────────────

#[test]
fn test_decode_garbage_fails() {
    let result = decode_record(&[0xFF, 0xFE, 0xFD]);
    assert!(result.is_err());
}

#[test]
fn test_decode_empty_fails() {
    let result = decode_record(&[]);
    assert!(result.is_err());
}

// ── Index key encoding ────────────────────────────────────────────────────

#[test]
fn test_index_key_typed_and_untyped_match() {
    // Typed and untyped values with the same payload must produce identical
    // index keys — otherwise lookups by `column.eq(value)` would miss rows
    // written through a different code path.
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
    // 42 (int) and "42" (string) should not collide in the index — even
    // though both could plausibly equal "42" textually, redb cares about
    // CBOR bytes.
    let int_key = value_to_index_key(&AnyRedbType::new(42i64)).unwrap();
    let str_key = value_to_index_key(&AnyRedbType::new("42".to_string())).unwrap();
    assert_ne!(int_key, str_key);
}
