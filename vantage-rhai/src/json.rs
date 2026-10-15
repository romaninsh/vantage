//! `Dynamic` ⇄ `serde_json::Value`.

use rhai::{Array, Dynamic, Map};
use serde_json::{Map as JsonMap, Number, Value};

/// Convert a rhai value to JSON, walking maps and arrays so an unsupported
/// value costs only its own leaf.
///
/// A leaf serde cannot represent — a custom type, a function pointer — renders
/// as its `to_string()`, so the maps and arrays around it keep their shape.
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
