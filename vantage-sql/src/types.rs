//! Shared CBOR ↔ JSON bridge functions.
//!
//! These conversions are lossy by design — JSON cannot represent CBOR tags,
//! binary data, or arbitrary-precision decimals. Used for interop with
//! serde-based code paths (e.g. AnyTable, struct deserialization).

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;

/// Convert a `serde_json::Value` into a `ciborium::Value`.
pub fn json_to_cbor(val: JsonValue) -> CborValue {
    match val {
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
                CborValue::Text(n.to_string())
            }
        }
        JsonValue::String(s) => CborValue::Text(s),
        JsonValue::Array(arr) => CborValue::Array(arr.into_iter().map(json_to_cbor).collect()),
        JsonValue::Object(map) => CborValue::Map(
            map.into_iter()
                .map(|(k, v)| (CborValue::Text(k), json_to_cbor(v)))
                .collect(),
        ),
    }
}

/// Convert a `ciborium::Value` into a `serde_json::Value`.
///
/// Lossy: CBOR tags are unwrapped, `Bytes` become hex or UTF-8 strings,
/// and large integers fall back to string representation.
pub fn cbor_to_json(val: CborValue) -> JsonValue {
    match val {
        CborValue::Null => JsonValue::Null,
        CborValue::Bool(b) => JsonValue::Bool(b),
        CborValue::Integer(i) => {
            let n = i128::from(i);
            if let Ok(v) = i64::try_from(n) {
                JsonValue::Number(v.into())
            } else if let Ok(v) = u64::try_from(n) {
                JsonValue::Number(v.into())
            } else {
                JsonValue::String(n.to_string())
            }
        }
        CborValue::Float(f) => serde_json::Number::from_f64(f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        CborValue::Text(s) => JsonValue::String(s),
        CborValue::Bytes(b) => match String::from_utf8(b) {
            Ok(s) => JsonValue::String(s),
            Err(e) => JsonValue::String(hex::encode(e.as_bytes())),
        },
        CborValue::Array(arr) => JsonValue::Array(arr.into_iter().map(cbor_to_json).collect()),
        CborValue::Map(map) => {
            let obj: serde_json::Map<String, JsonValue> = map
                .into_iter()
                .map(|(k, v)| {
                    let key = match k {
                        CborValue::Text(s) => s,
                        other => format!("{:?}", other),
                    };
                    (key, cbor_to_json(v))
                })
                .collect();
            JsonValue::Object(obj)
        }
        CborValue::Tag(10, inner) => {
            // Decimal — try to produce a JSON number, fall back to string
            if let CborValue::Text(s) = *inner {
                if let Ok(i) = s.parse::<i64>() {
                    JsonValue::Number(i.into())
                } else if let Ok(f) = s.parse::<f64>() {
                    serde_json::Number::from_f64(f)
                        .map(JsonValue::Number)
                        .unwrap_or(JsonValue::String(s))
                } else {
                    JsonValue::String(s)
                }
            } else {
                cbor_to_json(*inner)
            }
        }
        CborValue::Tag(_, inner) => cbor_to_json(*inner),
        _ => JsonValue::Null,
    }
}
