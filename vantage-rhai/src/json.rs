//! `Dynamic` ⇄ `serde_json::Value`. Replaces the hand-rolled converters in
//! consumers.

use rhai::{Array, Dynamic, Map};
use serde_json::{Map as JsonMap, Number, Value};

/// Convert a rhai value to JSON, walking maps and arrays so an unsupported
/// value costs only its own leaf.
///
/// A leaf serde cannot represent — a custom type, a function pointer — renders
/// as its `to_string()`. Doing this structurally matters: a single opaque value
/// nested in a map used to fail the whole conversion and flatten the entire
/// object into one string.
pub fn to_json(value: &Dynamic) -> Value {
    if value.is_unit() {
        return Value::Null;
    }
    if let Some(b) = value.clone().try_cast::<bool>() {
        return Value::Bool(b);
    }
    if let Some(i) = value.clone().try_cast::<i64>() {
        return Value::Number(i.into());
    }
    if let Some(f) = value.clone().try_cast::<f64>() {
        // JSON has no NaN or infinity; those fall through to text.
        return match Number::from_f64(f) {
            Some(n) => Value::Number(n),
            None => Value::String(f.to_string()),
        };
    }
    if let Some(s) = value.clone().try_cast::<rhai::ImmutableString>() {
        return Value::String(s.to_string());
    }
    if let Some(c) = value.clone().try_cast::<char>() {
        return Value::String(c.to_string());
    }
    if let Some(arr) = value.clone().try_cast::<Array>() {
        return Value::Array(arr.iter().map(to_json).collect());
    }
    if let Some(map) = value.clone().try_cast::<Map>() {
        let mut out = JsonMap::with_capacity(map.len());
        for (k, v) in map.iter() {
            out.insert(k.to_string(), to_json(v));
        }
        return Value::Object(out);
    }
    Value::String(value.to_string())
}

pub fn from_json(value: &Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::UNIT,
        Value::Bool(b) => Dynamic::from(*b),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => Dynamic::from(i),
            (None, Some(f)) => Dynamic::from(f),
            _ => Dynamic::from(n.to_string()),
        },
        Value::String(s) => Dynamic::from(s.clone()),
        Value::Array(a) => Dynamic::from(a.iter().map(from_json).collect::<Array>()),
        Value::Object(o) => {
            let mut map = Map::new();
            for (k, v) in o {
                map.insert(k.as_str().into(), from_json(v));
            }
            Dynamic::from(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Clone)]
    struct Opaque;

    #[test]
    fn round_trips_nested_values() {
        let v = json!({"a": [1, 2.5, true, null, "s"], "b": {"c": -3}});
        let d = from_json(&v);
        assert_eq!(to_json(&d), v);
    }

    #[test]
    fn custom_types_fall_back_to_display() {
        let d = Dynamic::from(Opaque);
        assert!(matches!(to_json(&d), Value::String(_)));
    }

    #[test]
    fn an_opaque_leaf_does_not_flatten_its_container() {
        // The shape around an unsupported value has to survive.
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
    }
}
