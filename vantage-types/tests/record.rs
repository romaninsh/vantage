use indexmap::IndexMap;
use vantage_types::{
    EmptyEntity, IntoRecord, InvariantValue, Record, TryFromRecord, TryIntoRecord,
};

fn record(pairs: &[(&str, &str)]) -> Record<String> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert(k.to_string(), v.to_string());
    }
    r
}

// construction and map access

#[test]
fn new_and_default_are_empty() {
    let r: Record<i32> = Record::new();
    assert!(r.is_empty());
    let d: Record<i32> = Record::default();
    assert_eq!(r, d);
}

#[test]
fn with_capacity_starts_empty() {
    let r: Record<i32> = Record::with_capacity(8);
    assert!(r.is_empty());
    assert!(r.as_inner().capacity() >= 8);
}

#[test]
fn fields_keep_insertion_order() {
    let mut r = Record::new();
    r.insert("b".to_string(), 1);
    r.insert("a".to_string(), 2);
    r.insert("c".to_string(), 3);
    let keys: Vec<&str> = r.keys().map(String::as_str).collect();
    assert_eq!(keys, ["b", "a", "c"]);
}

#[test]
fn deref_exposes_the_map_api() {
    let mut r = Record::new();
    r.insert("a".to_string(), 1);
    assert_eq!(r.len(), 1);
    assert_eq!(r.get("a"), Some(&1));
    assert_eq!(r["a"], 1);
    *r.get_mut("a").unwrap() = 5;
    assert_eq!(r["a"], 5);
}

#[test]
fn indexmap_conversions_round_trip() {
    let mut map = IndexMap::new();
    map.insert("x".to_string(), 1);
    map.insert("y".to_string(), 2);

    let r = Record::from_indexmap(map.clone());
    assert_eq!(r.as_inner(), &map);
    assert_eq!(r.clone().into_inner(), map);

    let from: Record<i32> = map.clone().into();
    assert_eq!(from, r);
    let back: IndexMap<String, i32> = from.into();
    assert_eq!(back, map);
}

#[test]
fn as_inner_mut_edits_the_record() {
    let mut r = Record::new();
    r.as_inner_mut().insert("a".to_string(), 1);
    assert_eq!(r["a"], 1);
}

#[test]
fn collects_from_pairs() {
    let r: Record<i32> = [("a".to_string(), 1), ("b".to_string(), 2)]
        .into_iter()
        .collect();
    assert_eq!(r.len(), 2);
    assert_eq!(r["b"], 2);
}

#[test]
fn iterates_by_value_reference_and_mutable_reference() {
    let mut r = Record::new();
    r.insert("a".to_string(), 1);
    r.insert("b".to_string(), 2);

    let by_ref: Vec<(&String, &i32)> = (&r).into_iter().collect();
    assert_eq!(by_ref.len(), 2);

    for (_, v) in &mut r {
        *v += 10;
    }
    assert_eq!(r["a"], 11);
    assert_eq!(r["b"], 12);

    let owned: Vec<(String, i32)> = r.into_iter().collect();
    assert_eq!(owned, [("a".to_string(), 11), ("b".to_string(), 12)]);
}

// changes_from

#[test]
fn only_the_differing_fields_come_back() {
    let before = record(&[
        ("status", "unregistered"),
        ("found", "[]"),
        ("verification_code", "123456"),
    ]);
    let after = record(&[("status", "pending"), ("found", "[]")]);

    let changes = after.changes_from(&before);

    assert_eq!(changes, record(&[("status", "pending")]));
}

// reshaping between value types

#[test]
fn into_record_converts_every_value() {
    let mut r: Record<i32> = Record::new();
    r.insert("a".to_string(), 1);
    r.insert("b".to_string(), 2);

    let wide: Record<i64> = r.into_record();

    assert_eq!(wide["a"], 1i64);
    assert_eq!(wide["b"], 2i64);
}

#[test]
fn try_into_record_between_value_types_cannot_fail() {
    let mut r: Record<i32> = Record::new();
    r.insert("a".to_string(), 1);

    let Ok(wide): Result<Record<i64>, std::convert::Infallible> = r.try_into_record();

    assert_eq!(wide["a"], 1i64);
}

#[test]
fn from_record_converts_with_try_from() {
    let mut r: Record<i32> = Record::new();
    r.insert("a".to_string(), 7);
    r.insert("b".to_string(), 200);

    let small = Record::<u8>::from_record(r).unwrap();

    assert_eq!(small["a"], 7u8);
    assert_eq!(small["b"], 200u8);
}

#[test]
fn from_record_fails_when_a_value_does_not_fit() {
    let mut r: Record<i32> = Record::new();
    r.insert("ok".to_string(), 1);
    r.insert("big".to_string(), 300);

    assert!(Record::<u8>::from_record(r).is_err());
}

#[test]
fn try_from_record_borrows_and_leaves_the_source_intact() {
    let mut r: Record<i32> = Record::new();
    r.insert("a".to_string(), 7);

    let small = Record::<u8>::try_from_record(&r).unwrap();

    assert_eq!(small["a"], 7u8);
    assert_eq!(r["a"], 7);
}

// EmptyEntity

#[test]
fn empty_entity_converts_to_an_empty_record() {
    let r: Record<i32> = EmptyEntity.into_record();
    assert!(r.is_empty());

    let Ok(r): Result<Record<String>, std::convert::Infallible> = EmptyEntity.try_into_record();
    assert!(r.is_empty());
}

#[test]
fn empty_entity_reads_from_any_record() {
    let mut r: Record<i32> = Record::new();
    r.insert("ignored".to_string(), 1);
    assert_eq!(EmptyEntity::from_record(r.clone()), Ok(EmptyEntity));
    assert_eq!(EmptyEntity::try_from_record(&r), Ok(EmptyEntity));
}

// InvariantValue

#[cfg(feature = "serde")]
#[test]
fn json_null_is_the_only_null() {
    assert!(serde_json::Value::Null.is_null());
    assert!(!serde_json::json!(0).is_null());
    assert!(!serde_json::json!("").is_null());
    assert!(!serde_json::json!(false).is_null());
}

#[cfg(feature = "serde")]
#[test]
fn json_value_eq_compares_representations() {
    assert!(serde_json::json!({"a": 1}).value_eq(&serde_json::json!({"a": 1})));
    assert!(!serde_json::json!(1).value_eq(&serde_json::json!("1")));
}

#[test]
fn cbor_null_is_the_only_null() {
    assert!(ciborium::Value::Null.is_null());
    assert!(!ciborium::Value::Integer(0.into()).is_null());
    assert!(!ciborium::Value::Text(String::new()).is_null());
}

#[test]
fn cbor_value_eq_compares_representations() {
    let a = ciborium::Value::Text("x".into());
    assert!(a.value_eq(&ciborium::Value::Text("x".into())));
    assert!(!a.value_eq(&ciborium::Value::Text("y".into())));
}

// serde-backed entities

#[cfg(feature = "serde")]
mod serde_entities {
    use super::*;
    use ciborium::Value as CborValue;
    use serde::{Deserialize, Serialize};
    use serde_json::{Value as JsonValue, json};
    use vantage_types::Entity;

    #[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
    struct User {
        name: String,
        age: i32,
    }

    fn assert_entity<E: Entity>() {}
    fn assert_cbor_entity<E: Entity<CborValue>>() {}

    #[test]
    fn serde_structs_are_entities_for_json_and_cbor() {
        assert_entity::<User>();
        assert_cbor_entity::<User>();
        assert_entity::<EmptyEntity>();
    }

    #[test]
    fn struct_converts_to_a_json_record_and_back() {
        let user = User {
            name: "Alice".into(),
            age: 30,
        };

        let record: Record<JsonValue> = user.clone().try_into_record().unwrap();

        assert_eq!(record.len(), 2);
        assert_eq!(record["name"], json!("Alice"));
        assert_eq!(record["age"], json!(30));
        assert_eq!(User::from_record(record).unwrap(), user);
    }

    #[test]
    fn json_object_converts_to_a_record_and_back() {
        let object = json!({"name": "Frank", "age": 45});

        let record: Record<JsonValue> = object.clone().into();

        assert_eq!(record["name"], json!("Frank"));
        assert_eq!(JsonValue::from(record), object);
    }

    #[test]
    fn json_non_object_is_wrapped_under_value() {
        let record: Record<JsonValue> = json!(5).into();
        assert_eq!(record.len(), 1);
        assert_eq!(record["value"], json!(5));

        let record: Record<JsonValue> = 5i32.try_into_record().unwrap();
        assert_eq!(record["value"], json!(5));
    }

    #[test]
    fn json_record_with_wrong_types_does_not_deserialize() {
        let mut record = Record::new();
        record.insert("name".to_string(), json!("Invalid"));
        record.insert("age".to_string(), json!("not a number"));
        assert!(User::from_record(record).is_err());
    }

    #[test]
    fn struct_converts_to_a_cbor_record_and_back() {
        let user = User {
            name: "Eve".into(),
            age: 28,
        };

        let record: Record<CborValue> = user.clone().try_into_record().unwrap();

        assert_eq!(record.len(), 2);
        assert_eq!(record["name"], CborValue::Text("Eve".into()));
        assert_eq!(record["age"], CborValue::Integer(28.into()));
        assert_eq!(User::from_record(record).unwrap(), user);
    }

    #[test]
    fn cbor_non_map_is_wrapped_under_value() {
        let record: Record<CborValue> = CborValue::Integer(5.into()).into();
        assert_eq!(record.len(), 1);
        assert_eq!(record["value"], CborValue::Integer(5.into()));

        let record: Record<CborValue> = 5i32.try_into_record().unwrap();
        assert_eq!(record["value"], CborValue::Integer(5.into()));
    }

    #[test]
    fn cbor_map_converts_to_a_record_and_back() {
        let map = CborValue::Map(vec![
            (CborValue::Text("a".into()), CborValue::Bool(true)),
            (CborValue::Text("b".into()), CborValue::Null),
        ]);

        let record: Record<CborValue> = map.clone().into();

        assert_eq!(record.len(), 2);
        assert_eq!(record["a"], CborValue::Bool(true));
        assert_eq!(CborValue::from(record), map);
    }

    #[test]
    fn cbor_record_with_wrong_types_does_not_deserialize() {
        let mut record = Record::new();
        record.insert("name".to_string(), CborValue::Text("x".into()));
        record.insert("age".to_string(), CborValue::Text("old".into()));
        assert!(User::from_record(record).is_err());
    }
}
