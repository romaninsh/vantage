//! `Dynamic` ⇄ `serde_json::Value` on rhai's serde support. Replaces the
//! hand-rolled converters in consumers.

use rhai::Dynamic;
use serde_json::Value;

/// Values serde cannot represent (custom types, function pointers) render as
/// their `to_string()`.
pub fn to_json(value: &Dynamic) -> Value {
    rhai::serde::from_dynamic::<Value>(value).unwrap_or_else(|_| Value::String(value.to_string()))
}

pub fn from_json(value: &Value) -> Dynamic {
    rhai::serde::to_dynamic(value).unwrap_or(Dynamic::UNIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_nested_values() {
        let v = json!({"a": [1, 2.5, true, null, "s"], "b": {"c": -3}});
        let d = from_json(&v);
        assert_eq!(to_json(&d), v);
    }

    #[test]
    fn custom_types_fall_back_to_display() {
        #[derive(Clone)]
        struct Opaque;
        let d = Dynamic::from(Opaque);
        assert!(matches!(to_json(&d), Value::String(_)));
    }
}
