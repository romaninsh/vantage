//! CBOR ↔ JSON bridge shared by every crate that renders `ciborium::Value`
//! as `serde_json::Value`.
//!
//! CBOR is a superset of JSON — tags, byte strings, non-text map keys,
//! integers beyond `u64` and non-finite floats have no JSON counterpart.
//! ciborium's serde bridge is no help here: `deserialize_any` on a
//! `Value::Tag(...)` calls `visit_enum`, which `serde_json::Value`'s
//! visitor rejects, so a serde round-trip fails on *any* tagged value
//! (and `.unwrap_or(Null)` then silently nulls the whole thing).
//!
//! Instead this module walks the value tree directly. Backends disagree
//! on how the JSON-less shapes should render (SurrealDB record ids become
//! `"table:id"`, SQL decimals become numbers, bytes are base64 in one
//! place and hex in another) — that per-backend policy lives in a
//! [`CborDialect`] implementation, while the recursive walk is written
//! once, here. [`PlainDialect`] is the neutral policy; backend crates
//! define their own dialect and override only the hooks that differ.
//!
//! JSON → CBOR needs no dialect: JSON is a strict subset of what CBOR
//! can represent, so [`json_to_cbor`] is total and lossless.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;

use crate::Record;

/// Rendering policy for CBOR shapes JSON can't represent directly.
///
/// Every method has a neutral default; a dialect overrides only the
/// hooks where its backend's rendering differs. Recursion happens by
/// calling back into [`cbor_to_json`] with the same dialect.
pub trait CborDialect {
    /// A tagged value. Default: drop the tag, render the payload.
    fn tag_to_json(&self, tag: u64, inner: CborValue) -> JsonValue {
        let _ = tag;
        cbor_to_json(self, inner)
    }

    /// A byte string. Default: lowercase hex.
    fn bytes_to_json(&self, bytes: Vec<u8>) -> JsonValue {
        JsonValue::String(hex_encode(&bytes))
    }

    /// A float. Default: finite → number, NaN/±Infinity → `null`
    /// (JSON has no representation for them).
    fn float_to_json(&self, f: f64) -> JsonValue {
        serde_json::Number::from_f64(f).map_or(JsonValue::Null, JsonValue::Number)
    }

    /// An integer outside both `i64` and `u64` (CBOR allows up to
    /// 64-bit magnitude either sign). Default: decimal string, so the
    /// digits survive.
    fn big_int_to_json(&self, n: i128) -> JsonValue {
        JsonValue::String(n.to_string())
    }

    /// A map key. JSON keys must be strings; CBOR keys can be anything.
    /// Default: text as-is, integers in decimal, anything else via Debug.
    fn map_key_to_string(&self, key: CborValue) -> String {
        match key {
            CborValue::Text(s) => s,
            CborValue::Integer(i) => i128::from(i).to_string(),
            other => format!("{other:?}"),
        }
    }
}

/// The neutral [`CborDialect`]: all defaults, no backend-specific tags.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlainDialect;

impl CborDialect for PlainDialect {}

/// Convert a CBOR value to JSON under the given dialect.
///
/// Total — never panics, never fails. What JSON cannot express is
/// rendered according to the dialect's hooks.
pub fn cbor_to_json<D: CborDialect + ?Sized>(dialect: &D, value: CborValue) -> JsonValue {
    match value {
        CborValue::Null => JsonValue::Null,
        CborValue::Bool(b) => JsonValue::Bool(b),
        CborValue::Integer(i) => {
            let n = i128::from(i);
            if let Ok(v) = i64::try_from(n) {
                JsonValue::Number(v.into())
            } else if let Ok(v) = u64::try_from(n) {
                JsonValue::Number(v.into())
            } else {
                dialect.big_int_to_json(n)
            }
        }
        CborValue::Float(f) => dialect.float_to_json(f),
        CborValue::Text(s) => JsonValue::String(s),
        CborValue::Bytes(b) => dialect.bytes_to_json(b),
        CborValue::Array(arr) => JsonValue::Array(
            arr.into_iter()
                .map(|v| cbor_to_json(dialect, v))
                .collect(),
        ),
        CborValue::Map(entries) => {
            let mut obj = serde_json::Map::with_capacity(entries.len());
            for (k, v) in entries {
                obj.insert(dialect.map_key_to_string(k), cbor_to_json(dialect, v));
            }
            JsonValue::Object(obj)
        }
        CborValue::Tag(tag, inner) => dialect.tag_to_json(tag, *inner),
        // ciborium::Value is #[non_exhaustive].
        _ => JsonValue::Null,
    }
}

/// Convert a whole record to a JSON object under the given dialect.
pub fn record_to_json<D: CborDialect + ?Sized>(
    dialect: &D,
    record: Record<CborValue>,
) -> JsonValue {
    let mut obj = serde_json::Map::with_capacity(record.len());
    for (k, v) in record {
        obj.insert(k, cbor_to_json(dialect, v));
    }
    JsonValue::Object(obj)
}

/// Render a CBOR value as a plain string: scalars bare (no JSON quoting),
/// `Null` empty, compound values as their JSON rendering under the
/// dialect. Used for ids, aggregates and form-encoded request values.
pub fn cbor_to_string<D: CborDialect + ?Sized>(dialect: &D, value: &CborValue) -> String {
    match value {
        CborValue::Text(s) => s.clone(),
        CborValue::Integer(i) => i128::from(*i).to_string(),
        CborValue::Float(f) => f.to_string(),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Null => String::new(),
        other => cbor_to_json(dialect, other.clone()).to_string(),
    }
}

/// Convert a JSON value to CBOR. Total and lossless — JSON is a strict
/// subset of what CBOR represents, so this cannot fail.
pub fn json_to_cbor(value: JsonValue) -> CborValue {
    match value {
        JsonValue::Null => CborValue::Null,
        JsonValue::Bool(b) => CborValue::Bool(b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                CborValue::Integer(i.into())
            } else if let Some(u) = n.as_u64() {
                CborValue::Integer(u.into())
            } else if let Some(f) = n.as_f64() {
                CborValue::Float(f)
            } else {
                // serde_json arbitrary-precision numbers beyond f64.
                CborValue::Text(n.to_string())
            }
        }
        JsonValue::String(s) => CborValue::Text(s),
        JsonValue::Array(arr) => {
            CborValue::Array(arr.into_iter().map(json_to_cbor).collect())
        }
        JsonValue::Object(map) => CborValue::Map(
            map.into_iter()
                .map(|(k, v)| (CborValue::Text(k), json_to_cbor(v)))
                .collect(),
        ),
    }
}

/// Lowercase hex, dependency-free.
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scalars_convert_directly() {
        assert_eq!(cbor_to_json(&PlainDialect, CborValue::Null), JsonValue::Null);
        assert_eq!(cbor_to_json(&PlainDialect, CborValue::Bool(true)), json!(true));
        assert_eq!(
            cbor_to_json(&PlainDialect, CborValue::Integer(42.into())),
            json!(42)
        );
        assert_eq!(
            cbor_to_json(&PlainDialect, CborValue::Float(1.5)),
            json!(1.5)
        );
        assert_eq!(
            cbor_to_json(&PlainDialect, CborValue::Text("hi".into())),
            json!("hi")
        );
    }

    #[test]
    fn u64_beyond_i64_stays_a_number() {
        assert_eq!(
            cbor_to_json(&PlainDialect, CborValue::Integer(u64::MAX.into())),
            json!(u64::MAX)
        );
    }

    #[test]
    fn big_int_becomes_decimal_string() {
        // ciborium integers span [-(2^64), 2^64-1]; only negatives below
        // i64::MIN fall outside both i64 and u64.
        let neg = i128::from(i64::MIN) - 1;
        assert_eq!(
            cbor_to_json(&PlainDialect, CborValue::Integer(neg.try_into().unwrap())),
            json!(neg.to_string())
        );
    }

    #[test]
    fn non_finite_floats_become_null() {
        assert_eq!(
            cbor_to_json(&PlainDialect, CborValue::Float(f64::NAN)),
            JsonValue::Null
        );
        assert_eq!(
            cbor_to_json(&PlainDialect, CborValue::Float(f64::INFINITY)),
            JsonValue::Null
        );
    }

    #[test]
    fn bytes_become_hex() {
        assert_eq!(
            cbor_to_json(&PlainDialect, CborValue::Bytes(vec![0xde, 0xad, 0x01])),
            json!("dead01")
        );
    }

    #[test]
    fn tag_unwraps_by_default() {
        let v = CborValue::Tag(999, Box::new(CborValue::Text("hi".into())));
        assert_eq!(cbor_to_json(&PlainDialect, v), json!("hi"));
    }

    #[test]
    fn tagged_value_survives_inside_a_map() {
        // The serde-bridge shortcut this module replaces failed on this
        // shape, collapsing the whole map to Null.
        let v = CborValue::Map(vec![(
            CborValue::Text("id".into()),
            CborValue::Tag(8, Box::new(CborValue::Text("user:1".into()))),
        )]);
        assert_eq!(cbor_to_json(&PlainDialect, v), json!({"id": "user:1"}));
    }

    #[test]
    fn map_keys_are_stringified() {
        let v = CborValue::Map(vec![
            (CborValue::Text("a".into()), CborValue::Bool(true)),
            (CborValue::Integer(7.into()), CborValue::Bool(false)),
        ]);
        assert_eq!(
            cbor_to_json(&PlainDialect, v),
            json!({"a": true, "7": false})
        );
    }

    #[test]
    fn dialect_hooks_override_rendering() {
        struct Base64ish;
        impl CborDialect for Base64ish {
            fn bytes_to_json(&self, bytes: Vec<u8>) -> JsonValue {
                JsonValue::String(format!("b64:{}", bytes.len()))
            }
            fn tag_to_json(&self, tag: u64, inner: CborValue) -> JsonValue {
                match (tag, inner) {
                    (8, CborValue::Text(s)) => JsonValue::String(format!("thing:{s}")),
                    (_, inner) => cbor_to_json(self, inner),
                }
            }
        }
        assert_eq!(
            cbor_to_json(&Base64ish, CborValue::Bytes(vec![1, 2, 3])),
            json!("b64:3")
        );
        assert_eq!(
            cbor_to_json(
                &Base64ish,
                CborValue::Tag(8, Box::new(CborValue::Text("x".into())))
            ),
            json!("thing:x")
        );
        // Unhandled tags still recurse *with the same dialect*.
        assert_eq!(
            cbor_to_json(
                &Base64ish,
                CborValue::Tag(1, Box::new(CborValue::Bytes(vec![1, 2])))
            ),
            json!("b64:2")
        );
    }

    #[test]
    fn json_round_trips_through_cbor() {
        let json = json!({
            "s": "text",
            "i": 42,
            "u": u64::MAX,
            "f": 1.25,
            "b": true,
            "n": null,
            "arr": [1, "two", {"nested": false}],
        });
        assert_eq!(
            cbor_to_json(&PlainDialect, json_to_cbor(json.clone())),
            json
        );
    }

    #[test]
    fn record_to_json_converts_every_field() {
        let mut record = Record::new();
        record.insert("name".to_string(), CborValue::Text("Alice".into()));
        record.insert(
            "id".to_string(),
            CborValue::Tag(8, Box::new(CborValue::Text("user:1".into()))),
        );
        assert_eq!(
            record_to_json(&PlainDialect, record),
            json!({"name": "Alice", "id": "user:1"})
        );
    }

    #[test]
    fn cbor_to_string_renders_scalars_bare() {
        assert_eq!(
            cbor_to_string(&PlainDialect, &CborValue::Text("abc".into())),
            "abc"
        );
        assert_eq!(
            cbor_to_string(&PlainDialect, &CborValue::Integer(42.into())),
            "42"
        );
        assert_eq!(
            cbor_to_string(&PlainDialect, &CborValue::Float(1.5)),
            "1.5"
        );
        assert_eq!(
            cbor_to_string(&PlainDialect, &CborValue::Bool(true)),
            "true"
        );
        assert_eq!(cbor_to_string(&PlainDialect, &CborValue::Null), "");
        assert_eq!(
            cbor_to_string(
                &PlainDialect,
                &CborValue::Array(vec![CborValue::Integer(1.into())])
            ),
            "[1]"
        );
    }
}
