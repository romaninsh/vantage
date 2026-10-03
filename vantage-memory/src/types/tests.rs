use super::*;

#[test]
fn scalars_round_trip() {
    let v = AnyMemoryType::from(42i64);
    assert_eq!(i64::try_from(v.clone()).unwrap(), 42);
    assert_eq!(v.value(), &ciborium::Value::Integer(42.into()));
    let s = AnyMemoryType::from("hi");
    assert_eq!(String::try_from(s).unwrap(), "hi");
    let b = AnyMemoryType::from(true);
    assert!(bool::try_from(b).unwrap());
}

#[test]
fn untyped_accepts_any_cbor_and_compares_by_value() {
    let a = AnyMemoryType::untyped(ciborium::Value::Integer(5.into()));
    let b = AnyMemoryType::from(5i64);
    assert_eq!(a, b);
    assert_eq!(a.type_variant(), None);
}

#[test]
fn option_none_is_null() {
    assert_eq!(None::<i64>.to_cbor(), ciborium::Value::Null);
}
