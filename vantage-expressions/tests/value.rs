//! `IntoValue`: scalars and lists convert to `serde_json::Value`.

use serde_json::{Value, json};
use vantage_expressions::value::IntoValue;

#[test]
fn scalars_become_json_scalars() {
    assert_eq!(Box::new(123i32).into_value(), json!(123));
    assert_eq!(Box::new(-5i64).into_value(), json!(-5));
    assert_eq!(Box::new(7u64).into_value(), json!(7));
    assert_eq!(Box::new(1.5f64).into_value(), json!(1.5));
    assert_eq!(Box::new(true).into_value(), json!(true));
    assert_eq!(Box::new("hello".to_string()).into_value(), json!("hello"));
    assert_eq!(Box::new("hello").into_value(), json!("hello"));
    assert_eq!(Box::new(()).into_value(), Value::Null);
}

#[test]
fn lists_and_arrays_become_json_arrays() {
    assert_eq!(
        Box::new(vec!["some", "slice"]).into_value(),
        json!(["some", "slice"])
    );
    assert_eq!(Box::new([1i32, 2, 3]).into_value(), json!([1, 2, 3]));
    assert_eq!(Box::new(Vec::<i32>::new()).into_value(), json!([]));
}

#[test]
fn non_finite_floats_become_null() {
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(Box::new(v).into_value(), Value::Null);
    }
}
