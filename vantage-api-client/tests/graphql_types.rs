//! `AnyGraphqlType`: a JSON value tagged with the GraphQL scalar it means,
//! so an `Int` is not a `BigInt` and a `String` is not a `DateTime`.

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime, Utc};
use indexmap::IndexMap;
use serde_json::{Value, json};
use uuid::Uuid;
use vantage_api_client::{AnyGraphqlType, GraphqlTypeVariants};
use vantage_expressions::{Expressive, ExpressiveEnum};

#[test]
fn bool_travels_as_a_bool() {
    let v = AnyGraphqlType::new(true);
    assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::Bool));
    assert_eq!(v.try_get::<bool>(), Some(true));
    assert_eq!(*v.value(), json!(true));
}

#[test]
fn i32_is_the_int_scalar() {
    let v = AnyGraphqlType::new(42i32);
    assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::Int));
    assert_eq!(v.try_get::<i32>(), Some(42));
    assert_eq!(*v.value(), json!(42));
}

#[test]
fn i64_is_the_bigint_scalar() {
    let v = AnyGraphqlType::new(9_000_000_000i64);
    assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::BigInt));
    assert_eq!(v.try_get::<i64>(), Some(9_000_000_000));
}

#[test]
fn f64_is_the_float_scalar() {
    let v = AnyGraphqlType::new(2.5f64);
    assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::Float));
    assert_eq!(v.try_get::<f64>(), Some(2.5));
    assert_eq!(*v.value(), json!(2.5));
}

#[test]
fn a_float_json_cannot_hold_becomes_null() {
    let v = AnyGraphqlType::new(f64::NAN);
    assert_eq!(*v.value(), Value::Null);
}

#[test]
fn string_is_the_string_scalar() {
    let v = AnyGraphqlType::new("hello".to_string());
    assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::String));
    assert_eq!(v.try_get::<String>(), Some("hello".to_string()));
}

#[test]
fn a_typed_value_is_not_coerced_to_another_type() {
    let text = AnyGraphqlType::new("not a number".to_string());
    assert_eq!(text.try_get::<i64>(), None);

    let moment = AnyGraphqlType::new("2026-05-16T12:00:00Z".parse::<DateTime<Utc>>().unwrap());
    assert_eq!(moment.try_get::<String>(), None);
    assert!(moment.try_get::<DateTime<Utc>>().is_some());
}

#[test]
fn an_untyped_value_leaves_the_parse_to_the_requested_type() {
    let number = AnyGraphqlType::untyped(json!(42));
    assert_eq!(number.type_variant(), None);
    assert_eq!(number.try_get::<i64>(), Some(42));

    let moment = AnyGraphqlType::untyped(json!("2026-05-16T12:00:00Z"));
    assert_eq!(
        moment.try_get::<DateTime<Utc>>(),
        Some("2026-05-16T12:00:00Z".parse().unwrap())
    );
}

#[test]
fn an_int_outside_32_bits_is_not_an_i32() {
    let big = AnyGraphqlType::untyped(json!(9_000_000_000i64));
    assert_eq!(big.try_get::<i32>(), None);
    assert_eq!(big.try_get::<i64>(), Some(9_000_000_000));
}

#[test]
fn a_bigint_sent_as_a_quoted_string_reads_as_i64() {
    let quoted = AnyGraphqlType::untyped(json!("9007199254740993"));
    assert_eq!(quoted.try_get::<i64>(), Some(9_007_199_254_740_993));
    assert_eq!(
        AnyGraphqlType::untyped(json!("12ab")).try_get::<i64>(),
        None
    );
}

#[test]
fn an_optional_value_is_null_when_absent() {
    let some = AnyGraphqlType::new(Some(42i64));
    assert_eq!(some.try_get::<Option<i64>>(), Some(Some(42)));

    let none = AnyGraphqlType::new(None::<i64>);
    assert_eq!(*none.value(), Value::Null);
    assert_eq!(none.try_get::<Option<i64>>(), Some(None));
}

#[test]
fn a_datetime_is_an_rfc3339_string() {
    let now: DateTime<Utc> = "2026-05-16T12:00:00Z".parse().unwrap();
    let v = AnyGraphqlType::new(now);
    assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::DateTime));
    assert_eq!(v.try_get::<DateTime<Utc>>(), Some(now));
    assert!(
        v.value()
            .as_str()
            .unwrap()
            .starts_with("2026-05-16T12:00:00")
    );
}

#[test]
fn a_datetime_with_an_offset_keeps_its_offset() {
    let at: DateTime<FixedOffset> = "2026-05-16T12:00:00+02:00".parse().unwrap();
    let v = AnyGraphqlType::new(at);
    assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::DateTime));
    assert_eq!(v.value().as_str(), Some("2026-05-16T12:00:00+02:00"));
    assert_eq!(v.try_get::<DateTime<FixedOffset>>(), Some(at));
}

#[test]
fn a_datetime_that_is_not_rfc3339_does_not_parse() {
    let v = AnyGraphqlType::untyped(json!("yesterday"));
    assert_eq!(v.try_get::<DateTime<Utc>>(), None);
    assert_eq!(
        AnyGraphqlType::untyped(json!(5)).try_get::<DateTime<Utc>>(),
        None
    );
}

#[test]
fn a_date_is_year_month_day() {
    let day = NaiveDate::from_ymd_opt(2026, 5, 16).unwrap();
    let v = AnyGraphqlType::new(day);
    assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::Date));
    assert_eq!(v.value().as_str(), Some("2026-05-16"));
    assert_eq!(v.try_get::<NaiveDate>(), Some(day));
    assert_eq!(
        AnyGraphqlType::untyped(json!("16/05/2026")).try_get::<NaiveDate>(),
        None
    );
}

#[test]
fn a_time_reads_with_or_without_fractional_seconds() {
    let whole = NaiveTime::from_hms_opt(12, 30, 45).unwrap();
    let v = AnyGraphqlType::new(whole);
    assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::Time));
    assert_eq!(v.try_get::<NaiveTime>(), Some(whole));

    let fractional = AnyGraphqlType::untyped(json!("12:30:45.250"));
    assert_eq!(
        fractional.try_get::<NaiveTime>(),
        NaiveTime::from_hms_milli_opt(12, 30, 45, 250)
    );
    assert_eq!(
        AnyGraphqlType::untyped(json!("noon")).try_get::<NaiveTime>(),
        None
    );
}

#[test]
fn a_uuid_is_its_hyphenated_string() {
    let id: Uuid = "550e8400-e29b-41d4-a716-446655440000".parse().unwrap();
    let v = AnyGraphqlType::new(id);
    assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::Uuid));
    assert_eq!(
        v.value().as_str(),
        Some("550e8400-e29b-41d4-a716-446655440000")
    );
    assert_eq!(v.try_get::<Uuid>(), Some(id));
    assert_eq!(
        AnyGraphqlType::untyped(json!("not-a-uuid")).try_get::<Uuid>(),
        None
    );
    assert_eq!(AnyGraphqlType::untyped(json!(7)).try_get::<Uuid>(), None);
}

#[test]
fn raw_json_passes_through_as_the_json_variant() {
    let raw = json!({ "a": [1, 2] });
    let v = AnyGraphqlType::new(raw.clone());
    assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::Json));
    assert_eq!(v.try_get::<Value>(), Some(raw));
}

#[test]
fn a_vec_is_an_array_and_a_map_is_an_object_in_order() {
    let list = vec![
        AnyGraphqlType::new(1i32),
        AnyGraphqlType::new("b".to_string()),
    ];
    let array = AnyGraphqlType::new(list);
    assert_eq!(array.type_variant(), Some(GraphqlTypeVariants::Array));
    assert_eq!(*array.value(), json!([1, "b"]));
    assert_eq!(array.try_get::<Vec<AnyGraphqlType>>().unwrap().len(), 2);

    let mut map = IndexMap::new();
    map.insert("z".to_string(), AnyGraphqlType::new(1i32));
    map.insert("a".to_string(), AnyGraphqlType::new(2i32));
    let object = AnyGraphqlType::new(map);
    assert_eq!(object.type_variant(), Some(GraphqlTypeVariants::Object));
    let keys: Vec<&String> = object.value().as_object().unwrap().keys().collect();
    assert_eq!(keys, ["z", "a"]);
    let back = object
        .try_get::<IndexMap<String, AnyGraphqlType>>()
        .unwrap();
    assert_eq!(back.keys().collect::<Vec<_>>(), ["z", "a"]);

    assert!(
        AnyGraphqlType::untyped(json!("x"))
            .try_get::<Vec<AnyGraphqlType>>()
            .is_none()
    );
    assert!(
        AnyGraphqlType::untyped(json!([1]))
            .try_get::<IndexMap<String, AnyGraphqlType>>()
            .is_none()
    );
}

#[test]
fn the_variant_is_guessed_from_raw_json() {
    let guess = |value: Value| GraphqlTypeVariants::from_json(&value);
    assert_eq!(guess(json!(null)), Some(GraphqlTypeVariants::Null));
    assert_eq!(guess(json!(false)), Some(GraphqlTypeVariants::Bool));
    assert_eq!(guess(json!(7)), Some(GraphqlTypeVariants::BigInt));
    assert_eq!(guess(json!(7.5)), Some(GraphqlTypeVariants::Float));
    assert_eq!(
        guess(json!("2026-05-16")),
        Some(GraphqlTypeVariants::String)
    );
    assert_eq!(guess(json!([])), Some(GraphqlTypeVariants::Array));
    assert_eq!(guess(json!({})), Some(GraphqlTypeVariants::Object));
}

#[test]
fn scalars_convert_in_and_out_of_the_any_type() {
    assert_eq!(*AnyGraphqlType::from(7i32).value(), json!(7));
    assert_eq!(*AnyGraphqlType::from(7i64).value(), json!(7));
    assert_eq!(*AnyGraphqlType::from(0.5f64).value(), json!(0.5));
    assert_eq!(*AnyGraphqlType::from(true).value(), json!(true));
    assert_eq!(*AnyGraphqlType::from("x").value(), json!("x"));
    assert_eq!(*AnyGraphqlType::from("x".to_string()).value(), json!("x"));

    assert_eq!(i32::try_from(AnyGraphqlType::from(7i32)).unwrap(), 7);
    assert_eq!(i64::try_from(AnyGraphqlType::from(7i64)).unwrap(), 7);
    assert_eq!(f64::try_from(AnyGraphqlType::from(0.5f64)).unwrap(), 0.5);
    assert!(bool::try_from(AnyGraphqlType::from(true)).unwrap());
    assert_eq!(String::try_from(AnyGraphqlType::from("x")).unwrap(), "x");

    let err = i64::try_from(AnyGraphqlType::from("x")).unwrap_err();
    assert!(err.to_string().contains("Cannot convert"), "{err}");
    let err = Vec::<AnyGraphqlType>::try_from(AnyGraphqlType::from("x")).unwrap_err();
    assert!(err.to_string().contains("Cannot convert"), "{err}");
    let list =
        Vec::<AnyGraphqlType>::try_from(AnyGraphqlType::new(vec![AnyGraphqlType::from(1i32)]));
    assert_eq!(list.unwrap().len(), 1);
}

#[test]
fn a_value_displays_as_its_json() {
    assert_eq!(AnyGraphqlType::from("x").to_string(), "\"x\"");
    assert_eq!(AnyGraphqlType::from(5i32).to_string(), "5");
}

#[test]
fn a_scalar_is_an_expression_parameter() {
    let expr = "x".expr();
    assert_eq!(expr.template, "{}");
    match &expr.parameters[..] {
        [ExpressiveEnum::Scalar(v)] => assert_eq!(*v.value(), json!("x")),
        other => panic!("expected one scalar parameter, got {other:?}"),
    }

    for expr in [
        Expressive::<AnyGraphqlType>::expr(&5i32),
        Expressive::<AnyGraphqlType>::expr(&5i64),
        Expressive::<AnyGraphqlType>::expr(&0.5f64),
        Expressive::<AnyGraphqlType>::expr(&true),
        Expressive::<AnyGraphqlType>::expr(&"s".to_string()),
        Expressive::<AnyGraphqlType>::expr(&AnyGraphqlType::from(1i32)),
    ] {
        assert_eq!(expr.template, "{}");
        assert_eq!(expr.parameters.len(), 1);
    }
}

#[test]
fn values_cross_to_and_from_cbor() {
    let cbor: ciborium::Value = AnyGraphqlType::from(42i64).into();
    assert_eq!(cbor, ciborium::Value::Integer(42.into()));

    let text: ciborium::Value = AnyGraphqlType::from("x").into();
    assert_eq!(text, ciborium::Value::Text("x".into()));

    let back = AnyGraphqlType::from(ciborium::Value::Text("x".into()));
    assert_eq!(*back.value(), json!("x"));
    let back = AnyGraphqlType::from(ciborium::Value::Bool(true));
    assert_eq!(*back.value(), json!(true));
}
