//! Lossy JSON↔CBOR transcoding used by the JSON convenience methods on `Engine`.
//!
//! `Engine::send_message_cbor` is the precision-preserving wire path. The
//! JSON-shaped `Engine::send_message` default routes through these helpers so
//! callers that build params with `serde_json::json!(...)` keep working over
//! the CBOR transport. Datetimes, durations, recordids and byte strings — the
//! cases CBOR carries faithfully and JSON cannot — are best-effort here. Use
//! `SurrealClient::query_cbor` when fidelity matters.
//!
//! The walk itself is `vantage_types::cbor_json`; this module only supplies
//! the rendering policy: bytes are encoded as base64 to match SurrealDB's
//! JSON wire format, record ids (`Tag(8)`) become `"table:id"`, NaN/inf
//! become `null` (JSON convention). u64 values above `i64::MAX` are
//! preserved on the way out (CBOR `Integer` carries them) and round-trip via
//! `serde_json::Number`'s arbitrary-precision support.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ciborium::Value as CborValue;
use serde_json::{Number, Value};
use vantage_types::cbor_json::{self, CborDialect};

/// Rendering policy for the JSON convenience path.
pub(crate) struct SurrealJsonDialect;

impl CborDialect for SurrealJsonDialect {
    fn bytes_to_json(&self, bytes: Vec<u8>) -> Value {
        Value::String(BASE64.encode(bytes))
    }

    /// Integers beyond `u64` (only negatives below `i64::MIN` in CBOR):
    /// best-effort f64, matching SurrealDB's own JSON output.
    fn big_int_to_json(&self, n: i128) -> Value {
        Number::from_f64(n as f64).map_or(Value::Null, Value::Number)
    }

    fn tag_to_json(&self, tag: u64, inner: CborValue) -> Value {
        // SurrealDB recordid: Tag(8, [table, id]) -> "table:id". Non-text
        // id parts (numeric/compound record ids) stringify, so `user:42`
        // still renders canonically.
        if tag == 8
            && let CborValue::Array(parts) = &inner
            && let [CborValue::Text(table), id] = parts.as_slice()
        {
            return match id {
                CborValue::Text(id) => Value::String(format!("{table}:{id}")),
                other => Value::String(format!(
                    "{table}:{}",
                    cbor_json::cbor_to_string(self, other)
                )),
            };
        }
        // Any other tag: drop it, render the payload.
        cbor_json::cbor_to_json(self, inner)
    }
}

pub(crate) fn json_to_cbor(value: Value) -> CborValue {
    cbor_json::json_to_cbor(value)
}

pub(crate) fn cbor_to_json(value: CborValue) -> Value {
    cbor_json::cbor_to_json(&SurrealJsonDialect, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn roundtrip(v: Value) -> Value {
        cbor_to_json(json_to_cbor(v))
    }

    #[test]
    fn primitives_round_trip() {
        assert_eq!(roundtrip(Value::Null), Value::Null);
        assert_eq!(roundtrip(json!(true)), json!(true));
        assert_eq!(roundtrip(json!("hello")), json!("hello"));
        assert_eq!(roundtrip(json!(42)), json!(42));
        assert_eq!(roundtrip(json!(-7)), json!(-7));
        assert_eq!(roundtrip(json!(3.5)), json!(3.5));
    }

    #[test]
    fn nested_round_trip() {
        let v = json!({
            "id": "users:john",
            "tags": ["a", "b", "c"],
            "meta": { "active": true, "count": 12 }
        });
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn surreal_query_response_shape() {
        let v = json!([
            { "status": "OK", "result": [{ "id": "users:1", "name": "Alice" }] }
        ]);
        assert_eq!(roundtrip(v.clone()), v);
    }

    #[test]
    fn numeric_edges() {
        assert_eq!(roundtrip(json!(i64::MAX)), json!(i64::MAX));
        assert_eq!(roundtrip(json!(i64::MIN)), json!(i64::MIN));
        assert_eq!(roundtrip(json!(u64::MAX)), json!(u64::MAX));
    }

    #[test]
    fn nan_and_inf_become_null() {
        assert_eq!(cbor_to_json(CborValue::Float(f64::NAN)), Value::Null);
        assert_eq!(cbor_to_json(CborValue::Float(f64::INFINITY)), Value::Null);
    }

    #[test]
    fn cbor_bytes_become_base64() {
        let cbor = CborValue::Bytes(vec![0x68, 0x69]);
        assert_eq!(cbor_to_json(cbor), Value::String("aGk=".to_string()));
    }

    #[test]
    fn cbor_tag_unwraps_to_inner() {
        let cbor = CborValue::Tag(0, Box::new(CborValue::Text("2024-01-01".to_string())));
        assert_eq!(cbor_to_json(cbor), Value::String("2024-01-01".to_string()));
    }

    #[test]
    fn record_id_becomes_table_colon_id() {
        let cbor = CborValue::Tag(
            8,
            Box::new(CborValue::Array(vec![
                CborValue::Text("users".into()),
                CborValue::Text("john".into()),
            ])),
        );
        assert_eq!(cbor_to_json(cbor), Value::String("users:john".to_string()));
    }

    #[test]
    fn numeric_record_id_stringifies() {
        let cbor = CborValue::Tag(
            8,
            Box::new(CborValue::Array(vec![
                CborValue::Text("users".into()),
                CborValue::Integer(42.into()),
            ])),
        );
        assert_eq!(cbor_to_json(cbor), Value::String("users:42".to_string()));
    }
}
