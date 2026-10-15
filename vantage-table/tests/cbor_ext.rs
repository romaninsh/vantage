//! `CborValueExt`: the short accessor names and string-keyed map lookup that
//! `ciborium::Value` lacks.

use ciborium::Value as CborValue;
use vantage_table::prelude::CborValueExt;

fn map(pairs: Vec<(&str, CborValue)>) -> CborValue {
    CborValue::Map(
        pairs
            .into_iter()
            .map(|(k, v)| (CborValue::Text(k.to_string()), v))
            .collect(),
    )
}

#[test]
fn as_str_borrows_a_text_value_and_is_none_for_anything_else() {
    assert_eq!(CborValue::Text("hello".into()).as_str(), Some("hello"));
    assert_eq!(CborValue::Integer(1.into()).as_str(), None);
    assert_eq!(CborValue::Null.as_str(), None);
}

#[test]
fn as_i64_reads_an_integer_that_fits() {
    assert_eq!(CborValue::Integer(42i64.into()).as_i64(), Some(42));
    assert_eq!(CborValue::Integer((-7i64).into()).as_i64(), Some(-7));
}

#[test]
fn as_i64_is_none_for_a_non_integer_or_one_that_does_not_fit() {
    assert_eq!(CborValue::Text("1".into()).as_i64(), None);
    assert_eq!(CborValue::Integer(u64::MAX.into()).as_i64(), None);
}

#[test]
fn as_u64_reads_an_integer_that_fits() {
    assert_eq!(CborValue::Integer(42u64.into()).as_u64(), Some(42));
    assert_eq!(CborValue::Integer(u64::MAX.into()).as_u64(), Some(u64::MAX));
}

#[test]
fn as_u64_is_none_for_a_non_integer_or_a_negative_one() {
    assert_eq!(CborValue::Bool(true).as_u64(), None);
    assert_eq!(CborValue::Integer((-1i64).into()).as_u64(), None);
}

#[test]
fn as_f64_reads_a_float_only() {
    assert_eq!(CborValue::Float(1.5).as_f64(), Some(1.5));
    assert_eq!(CborValue::Integer(1.into()).as_f64(), None);
}

#[test]
fn get_looks_a_value_up_by_its_string_key() {
    let record = map(vec![
        ("name", CborValue::Text("alice".into())),
        ("age", CborValue::Integer(30i64.into())),
    ]);
    assert_eq!(record.get("name").and_then(|v| v.as_str()), Some("alice"));
    assert_eq!(record.get("age").and_then(|v| v.as_i64()), Some(30));
}

#[test]
fn get_is_none_for_a_missing_key_or_a_value_that_is_not_a_map() {
    let record = map(vec![("name", CborValue::Text("alice".into()))]);
    assert!(record.get("missing").is_none());
    assert!(CborValue::Text("alice".into()).get("name").is_none());
    assert!(CborValue::Array(vec![]).get("name").is_none());
}

#[test]
fn get_ignores_entries_whose_key_is_not_text() {
    let record = CborValue::Map(vec![(
        CborValue::Integer(1.into()),
        CborValue::Text("one".into()),
    )]);
    assert!(record.get("1").is_none());
}

#[test]
fn get_mut_lets_the_caller_change_a_value_in_place() {
    let mut record = map(vec![("age", CborValue::Integer(30i64.into()))]);
    *record.get_mut("age").expect("age") = CborValue::Integer(31i64.into());
    assert_eq!(record.get("age").and_then(|v| v.as_i64()), Some(31));
}

#[test]
fn get_mut_is_none_for_a_missing_key_or_a_value_that_is_not_a_map() {
    let mut record = map(vec![("age", CborValue::Integer(30i64.into()))]);
    assert!(record.get_mut("missing").is_none());
    assert!(CborValue::Null.get_mut("age").is_none());
}
