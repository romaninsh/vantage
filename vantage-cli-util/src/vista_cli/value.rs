//! Value coercion for CLI condition arguments.
//!
//! Two paths:
//! - **Auto-detect** (default): try integer → float → bool → text. Keeps
//!   the common `field=42`, `field=3.14`, `field=true` ergonomics.
//! - **Typed JSON** (`#`-prefixed): forces the value to be parsed as a
//!   JSON literal, which preserves bool/int/float/null/array/map types
//!   unambiguously. Necessary when a string happens to look like a
//!   number or boolean, or when a heterogeneous list / nested map is
//!   needed.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};

/// Coerce a raw CLI value string to CBOR. `#`-prefixed values are parsed
/// as JSON literals; everything else goes through the auto-detect path.
pub fn parse_value(s: &str) -> Result<CborValue> {
    if let Some(rest) = s.strip_prefix('#') {
        let json: serde_json::Value = serde_json::from_str(rest)
            .map_err(|e| error!(format!("invalid JSON literal `{rest}`: {e}")))?;
        Ok(json_to_cbor(json))
    } else {
        Ok(auto_detect(s))
    }
}

/// Cheap heuristic typing for unprefixed values: integer if it parses,
/// then float, then `true`/`false`, else text. Drivers translate
/// further at their own boundary.
pub fn auto_detect(value: &str) -> CborValue {
    if let Ok(i) = value.parse::<i64>() {
        CborValue::Integer(i.into())
    } else if let Ok(f) = value.parse::<f64>() {
        CborValue::Float(f)
    } else if value == "true" {
        CborValue::Bool(true)
    } else if value == "false" {
        CborValue::Bool(false)
    } else {
        CborValue::Text(value.to_string())
    }
}

/// Translate a `serde_json::Value` into the matching `ciborium::Value`.
/// Lossless for the JSON type set (the JSON Number's int-vs-float
/// distinction maps onto CBOR's `Integer` vs `Float`).
pub fn json_to_cbor(j: serde_json::Value) -> CborValue {
    use serde_json::Value as J;
    match j {
        J::Null => CborValue::Null,
        J::Bool(b) => CborValue::Bool(b),
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                CborValue::Integer(i.into())
            } else if let Some(u) = n.as_u64() {
                // u64 in (i64::MAX, u64::MAX]; `ciborium::value::Integer`
                // takes a `u64` directly.
                CborValue::Integer(u.into())
            } else if let Some(f) = n.as_f64() {
                CborValue::Float(f)
            } else {
                CborValue::Null
            }
        }
        J::String(s) => CborValue::Text(s),
        J::Array(arr) => CborValue::Array(arr.into_iter().map(json_to_cbor).collect()),
        J::Object(map) => CborValue::Map(
            map.into_iter()
                .map(|(k, v)| (CborValue::Text(k), json_to_cbor(v)))
                .collect(),
        ),
    }
}

/// Parse a comma-separated value list for `field:in=a,b,c`. Each
/// element goes through `parse_value` so `#`-typed elements work
/// (`#1,#2,#"three"` etc., though `#[1,2,"three"]` is usually cleaner).
pub fn parse_value_list(s: &str) -> Result<Vec<CborValue>> {
    if let Some(rest) = s.strip_prefix('#') {
        // The whole list is a JSON array.
        let json: serde_json::Value = serde_json::from_str(rest)
            .map_err(|e| error!(format!("invalid JSON array `{rest}`: {e}")))?;
        match json {
            serde_json::Value::Array(arr) => Ok(arr.into_iter().map(json_to_cbor).collect()),
            other => Err(error!(format!(
                "`:in=#…` expects a JSON array, got `{other}`"
            ))),
        }
    } else {
        Ok(s.split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(auto_detect)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autodetect_kinds() {
        assert!(matches!(auto_detect("42"), CborValue::Integer(_)));
        assert!(matches!(auto_detect("3.14"), CborValue::Float(_)));
        assert_eq!(auto_detect("true"), CborValue::Bool(true));
        assert_eq!(auto_detect("false"), CborValue::Bool(false));
        assert_eq!(auto_detect("alice"), CborValue::Text("alice".into()));
    }

    #[test]
    fn typed_json_overrides_autodetect() {
        // Without `#`, "42" is an int.
        assert!(matches!(parse_value("42").unwrap(), CborValue::Integer(_)));
        // With `#"42"`, it's a string.
        assert_eq!(
            parse_value("#\"42\"").unwrap(),
            CborValue::Text("42".into())
        );
    }

    #[test]
    fn typed_json_bool_int_null() {
        assert_eq!(parse_value("#true").unwrap(), CborValue::Bool(true));
        assert!(matches!(parse_value("#42").unwrap(), CborValue::Integer(_)));
        assert_eq!(parse_value("#null").unwrap(), CborValue::Null);
    }

    #[test]
    fn typed_json_array() {
        let v = parse_value("#[1, 2, \"three\"]").unwrap();
        match v {
            CborValue::Array(items) => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[2], CborValue::Text("three".into()));
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn typed_json_object() {
        let v = parse_value("#{\"nested\": \"obj\"}").unwrap();
        match v {
            CborValue::Map(pairs) => {
                assert_eq!(pairs.len(), 1);
                assert_eq!(pairs[0].0, CborValue::Text("nested".into()));
                assert_eq!(pairs[0].1, CborValue::Text("obj".into()));
            }
            other => panic!("expected Map, got {other:?}"),
        }
    }

    #[test]
    fn typed_json_bad_input() {
        assert!(parse_value("#{bad json").is_err());
    }

    #[test]
    fn value_list_split() {
        let xs = parse_value_list("a,b,c").unwrap();
        assert_eq!(xs.len(), 3);
        assert_eq!(xs[0], CborValue::Text("a".into()));

        let nums = parse_value_list("1,2,3").unwrap();
        assert!(matches!(nums[0], CborValue::Integer(_)));
    }

    #[test]
    fn value_list_json_array() {
        let xs = parse_value_list("#[1, \"two\", true]").unwrap();
        assert_eq!(xs.len(), 3);
        assert_eq!(xs[2], CborValue::Bool(true));
    }
}
