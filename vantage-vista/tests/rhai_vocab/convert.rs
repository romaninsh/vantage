use ciborium::Value as CborValue;
use vantage_rhai::rhai::{Array, Dynamic, Map as RhaiMap};
use vantage_vista::rhai::{
    cbor_to_dynamic, dynamic_to_cbor, map_to_record, record_to_dynamic, record_to_map,
};

use super::support::rec;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

#[test]
fn scalars_cross_to_cbor_and_back() {
    assert_eq!(dynamic_to_cbor(Dynamic::UNIT).unwrap(), CborValue::Null);
    assert_eq!(
        dynamic_to_cbor(Dynamic::from(true)).unwrap(),
        CborValue::Bool(true)
    );
    assert_eq!(
        dynamic_to_cbor(Dynamic::from(7_i64)).unwrap(),
        CborValue::Integer(7.into())
    );
    assert_eq!(
        dynamic_to_cbor(Dynamic::from(2.5_f64)).unwrap(),
        CborValue::Float(2.5)
    );
    assert_eq!(
        dynamic_to_cbor(Dynamic::from("x".to_string())).unwrap(),
        text("x")
    );
    assert_eq!(dynamic_to_cbor(Dynamic::from('c')).unwrap(), text("c"));
    assert_eq!(
        dynamic_to_cbor(Dynamic::from_blob(vec![1, 2])).unwrap(),
        CborValue::Bytes(vec![1, 2])
    );

    assert!(cbor_to_dynamic(&CborValue::Null).is_unit());
    assert!(cbor_to_dynamic(&CborValue::Bool(true)).as_bool().unwrap());
    assert_eq!(
        cbor_to_dynamic(&CborValue::Integer(9.into()))
            .as_int()
            .unwrap(),
        9
    );
    assert_eq!(
        cbor_to_dynamic(&CborValue::Float(1.5)).as_float().unwrap(),
        1.5
    );
    assert_eq!(cbor_to_dynamic(&text("hi")).into_string().unwrap(), "hi");
    assert!(cbor_to_dynamic(&CborValue::Bytes(vec![1])).is_blob());
}

#[test]
fn a_record_id_renders_as_table_colon_id_and_a_datetime_as_its_text() {
    let thing = CborValue::Tag(8, Box::new(CborValue::Array(vec![text("user"), text("1")])));
    assert_eq!(cbor_to_dynamic(&thing).into_string().unwrap(), "user:1");
    let dt = CborValue::Tag(0, Box::new(text("2026-01-01T00:00:00Z")));
    assert_eq!(
        cbor_to_dynamic(&dt).into_string().unwrap(),
        "2026-01-01T00:00:00Z"
    );
}

#[test]
fn an_integer_beyond_i64_becomes_a_decimal_string_not_a_wraparound() {
    let n = i128::from(i64::MIN) - 1;
    let big = CborValue::Integer(n.try_into().unwrap());
    assert_eq!(cbor_to_dynamic(&big).into_string().unwrap(), n.to_string());
}

#[test]
fn arrays_and_maps_round_trip() {
    let mut map = RhaiMap::new();
    map.insert(
        "ids".into(),
        Dynamic::from(vec![Dynamic::from(1_i64), Dynamic::from("x".to_string())]),
    );
    let cbor = dynamic_to_cbor(Dynamic::from(map)).unwrap();
    let CborValue::Map(pairs) = &cbor else {
        panic!("{cbor:?}")
    };
    assert_eq!(pairs[0].0, text("ids"));
    assert!(matches!(&pairs[0].1, CborValue::Array(a) if a.len() == 2));
    let back = cbor_to_dynamic(&cbor);
    assert!(back.is_map());
    let items = back
        .cast::<RhaiMap>()
        .remove("ids")
        .unwrap()
        .into_array()
        .unwrap();
    assert_eq!(items.len(), 2);
    let _: Array = items;
}

#[test]
fn an_instant_travels_as_a_cbor_datetime() {
    let at = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(
        dynamic_to_cbor(Dynamic::from(at)).unwrap(),
        CborValue::Tag(0, Box::new(text(&at.to_rfc3339())))
    );
}

#[test]
fn a_value_with_no_cbor_story_is_an_error_naming_its_type() {
    #[derive(Clone)]
    struct Opaque;
    let err = dynamic_to_cbor(Dynamic::from(Opaque)).unwrap_err();
    assert!(err.to_string().contains("Opaque"), "{err}");

    let nested = Dynamic::from(vec![Dynamic::from(Opaque)]);
    assert!(dynamic_to_cbor(nested).is_err());
}

#[test]
fn records_convert_to_rhai_maps_and_back() {
    let record = rec(&[("a", 1), ("b", 2)]);
    let map = record_to_map(&record);
    assert_eq!(map.len(), 2);
    assert_eq!(map["a"].as_int().unwrap(), 1);
    assert!(record_to_dynamic(&record).is_map());
    assert_eq!(map_to_record(map).unwrap(), record);

    let mut bad = RhaiMap::new();
    #[derive(Clone)]
    struct Opaque;
    bad.insert("x".into(), Dynamic::from(Opaque));
    assert!(map_to_record(bad).is_err());
}
