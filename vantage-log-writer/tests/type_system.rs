use serde_json::{Value, json};
use vantage_log_writer::{AnyJsonType, JsonType, JsonTypeVariants};

#[test]
fn values_round_trip_through_any_json_type() {
    let any = AnyJsonType::new("hello".to_string());
    assert_eq!(any.type_variant(), Some(JsonTypeVariants::String));
    assert_eq!(any.try_get::<String>(), Some("hello".to_string()));

    let any = AnyJsonType::new(42_i64);
    assert_eq!(any.type_variant(), Some(JsonTypeVariants::Int));
    assert_eq!(any.try_get::<i64>(), Some(42));

    let any = AnyJsonType::new(1.5_f64);
    assert_eq!(any.type_variant(), Some(JsonTypeVariants::Float));
    assert_eq!(any.try_get::<f64>(), Some(1.5));

    let any = AnyJsonType::new(true);
    assert_eq!(any.type_variant(), Some(JsonTypeVariants::Bool));
    assert_eq!(any.try_get::<bool>(), Some(true));
}

#[test]
fn variants_are_detected_from_json_values() {
    assert_eq!(
        JsonTypeVariants::from_json(&Value::Null),
        Some(JsonTypeVariants::Null)
    );
    assert_eq!(
        JsonTypeVariants::from_json(&json!(true)),
        Some(JsonTypeVariants::Bool)
    );
    assert_eq!(
        JsonTypeVariants::from_json(&json!(7)),
        Some(JsonTypeVariants::Int)
    );
    assert_eq!(
        JsonTypeVariants::from_json(&json!(7.5)),
        Some(JsonTypeVariants::Float)
    );
    assert_eq!(
        JsonTypeVariants::from_json(&json!("x")),
        Some(JsonTypeVariants::String)
    );
    assert_eq!(
        JsonTypeVariants::from_json(&json!([1, 2])),
        Some(JsonTypeVariants::Array)
    );
    assert_eq!(
        JsonTypeVariants::from_json(&json!({"a": 1})),
        Some(JsonTypeVariants::Object)
    );
}

#[test]
fn json_conversions_encode_and_decode_each_type() {
    assert_eq!("a".to_string().to_json(), json!("a"));
    assert_eq!(
        <String as JsonType>::from_json(json!("a")),
        Some("a".into())
    );
    assert_eq!(<String as JsonType>::from_json(json!(1)), None);

    assert_eq!(5_i64.to_json(), json!(5));
    assert_eq!(<i64 as JsonType>::from_json(json!(5)), Some(5));
    assert_eq!(<i64 as JsonType>::from_json(json!("5")), None);

    assert_eq!(2.5_f64.to_json(), json!(2.5));
    assert_eq!(<f64 as JsonType>::from_json(json!(2.5)), Some(2.5));
    assert_eq!(<f64 as JsonType>::from_json(json!(false)), None);

    assert_eq!(false.to_json(), json!(false));
    assert_eq!(<bool as JsonType>::from_json(json!(true)), Some(true));
    assert_eq!(<bool as JsonType>::from_json(json!(0)), None);

    let nested = json!({"a": [1, 2]});
    assert_eq!(nested.to_json(), nested);
    assert_eq!(<Value as JsonType>::from_json(nested.clone()), Some(nested));
}

#[test]
fn option_maps_none_to_null_and_back() {
    assert_eq!(Some(3_i64).to_json(), json!(3));
    assert_eq!(None::<i64>.to_json(), Value::Null);
    assert_eq!(
        <Option<i64> as JsonType>::from_json(Value::Null),
        Some(None)
    );
    assert_eq!(
        <Option<i64> as JsonType>::from_json(json!(3)),
        Some(Some(3))
    );
    assert_eq!(<Option<i64> as JsonType>::from_json(json!("x")), None);
}
