use serde_json::{Value, json};
use vantage_rhai::rhai::{Array, Dynamic, Map};
use vantage_rhai::{from_json, to_json};

#[derive(Clone)]
struct Opaque;

#[test]
fn nested_json_survives_a_round_trip() {
    let v = json!({"a": [1, 2.5, true, null, "s"], "b": {"c": -3}});
    assert_eq!(to_json(&from_json(&v)), v);
}

#[test]
fn null_is_unit_and_back() {
    assert!(from_json(&Value::Null).is_unit());
    assert_eq!(to_json(&Dynamic::UNIT), Value::Null);
}

#[test]
fn booleans_map_both_ways() {
    assert!(from_json(&json!(true)).as_bool().unwrap());
    assert_eq!(to_json(&Dynamic::from(false)), json!(false));
}

#[test]
fn whole_numbers_become_integers() {
    let d = from_json(&json!(42));
    assert_eq!(d.as_int().unwrap(), 42);
    assert_eq!(to_json(&Dynamic::from(-7_i64)), json!(-7));
}

#[test]
fn fractional_numbers_become_floats() {
    let d = from_json(&json!(2.5));
    assert_eq!(d.as_float().unwrap(), 2.5);
    assert_eq!(to_json(&Dynamic::from(2.5_f64)), json!(2.5));
}

#[test]
fn strings_map_both_ways() {
    assert_eq!(from_json(&json!("hi")).into_string().unwrap(), "hi");
    assert_eq!(to_json(&Dynamic::from("hi".to_string())), json!("hi"));
}

#[test]
fn a_char_becomes_a_one_character_string() {
    assert_eq!(to_json(&Dynamic::from('x')), json!("x"));
}

#[test]
fn arrays_map_element_by_element() {
    let d = from_json(&json!([1, "two", [3]]));
    let items = d.cast::<Array>();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].as_int().unwrap(), 1);
    assert_eq!(items[1].clone().into_string().unwrap(), "two");
    assert_eq!(to_json(&Dynamic::from(items)), json!([1, "two", [3]]));
}

#[test]
fn objects_map_key_by_key() {
    let d = from_json(&json!({"name": "Ada", "age": 36}));
    let map = d.cast::<Map>();
    assert_eq!(map["name"].clone().into_string().unwrap(), "Ada");
    assert_eq!(map["age"].as_int().unwrap(), 36);
    assert_eq!(
        to_json(&Dynamic::from(map)),
        json!({"name": "Ada", "age": 36})
    );
}

#[test]
fn an_empty_array_and_an_empty_object_stay_distinct() {
    assert_eq!(to_json(&from_json(&json!([]))), json!([]));
    assert_eq!(to_json(&from_json(&json!({}))), json!({}));
}

#[test]
fn a_custom_type_renders_as_a_string() {
    assert!(matches!(to_json(&Dynamic::from(Opaque)), Value::String(_)));
}

#[test]
fn an_opaque_leaf_costs_only_itself_not_the_container_around_it() {
    let mut map = Map::new();
    map.insert("kept".into(), Dynamic::from(7_i64));
    map.insert("opaque".into(), Dynamic::from(Opaque));
    map.insert(
        "nested".into(),
        Dynamic::from(vec![Dynamic::from(1_i64), Dynamic::from(Opaque)]),
    );

    let out = to_json(&Dynamic::from(map));
    let Value::Object(o) = &out else {
        panic!("expected an object, got {out}")
    };
    assert_eq!(o["kept"], json!(7));
    assert!(matches!(o["opaque"], Value::String(_)));
    let Value::Array(a) = &o["nested"] else {
        panic!("expected an array, got {}", o["nested"])
    };
    assert_eq!(a[0], json!(1));
    assert!(matches!(a[1], Value::String(_)));
}

#[test]
fn non_finite_floats_become_text() {
    assert!(matches!(
        to_json(&Dynamic::from(f64::NAN)),
        Value::String(_)
    ));
    assert!(matches!(
        to_json(&Dynamic::from(f64::INFINITY)),
        Value::String(_)
    ));
}

#[test]
fn a_script_result_converts_to_json() {
    let host = vantage_rhai::ui_host();
    let value = host
        .compile(&vantage_rhai::Block::from(
            r#"#{ vips: 3, ids: [1, 2], label: "x" }"#,
        ))
        .unwrap()
        .eval(&vantage_rhai::Env::new())
        .unwrap();
    assert_eq!(
        to_json(&value),
        json!({"vips": 3, "ids": [1, 2], "label": "x"})
    );
}
