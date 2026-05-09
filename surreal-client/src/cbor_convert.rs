//! Lossy JSON↔CBOR transcoding used by the JSON convenience methods on `Engine`.
//!
//! `Engine::send_message_cbor` is the precision-preserving wire path. The
//! JSON-shaped `Engine::send_message` default routes through these helpers so
//! callers that build params with `serde_json::json!(...)` keep working over
//! the CBOR transport. Datetimes, durations, recordids and byte strings — the
//! cases CBOR carries faithfully and JSON cannot — are best-effort here. Use
//! `SurrealClient::query_cbor` when fidelity matters.
//!
//! Bytes are encoded as base64 to match SurrealDB's JSON wire format.
//! NaN/inf become `null` (JSON convention). u64 values above `i64::MAX` are
//! preserved on the way out (CBOR `Integer` carries them) and round-trip via
//! `serde_json::Number`'s arbitrary-precision support.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ciborium::Value as CborValue;
use ciborium::value::Integer;
use serde_json::{Map, Number, Value};

pub(crate) fn json_to_cbor(value: Value) -> CborValue {
    match value {
        Value::Null => CborValue::Null,
        Value::Bool(b) => CborValue::Bool(b),
        Value::Number(n) => number_to_cbor(n),
        Value::String(s) => CborValue::Text(s),
        Value::Array(arr) => CborValue::Array(arr.into_iter().map(json_to_cbor).collect()),
        Value::Object(obj) => CborValue::Map(
            obj.into_iter()
                .map(|(k, v)| (CborValue::Text(k), json_to_cbor(v)))
                .collect(),
        ),
    }
}

pub(crate) fn cbor_to_json(value: CborValue) -> Value {
    match value {
        CborValue::Null => Value::Null,
        CborValue::Bool(b) => Value::Bool(b),
        CborValue::Integer(i) => integer_to_json(i),
        CborValue::Float(f) => Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null),
        CborValue::Text(s) => Value::String(s),
        CborValue::Bytes(b) => Value::String(BASE64.encode(b)),
        CborValue::Array(arr) => Value::Array(arr.into_iter().map(cbor_to_json).collect()),
        CborValue::Map(entries) => {
            let mut obj = Map::with_capacity(entries.len());
            for (k, v) in entries {
                let key = match k {
                    CborValue::Text(s) => s,
                    other => format!("{:?}", other),
                };
                obj.insert(key, cbor_to_json(v));
            }
            Value::Object(obj)
        }
        // SurrealDB recordid: Tag(8, [table, id]) -> "table:id"
        CborValue::Tag(8, inner) => {
            if let CborValue::Array(parts) = inner.as_ref()
                && parts.len() == 2
                && let (CborValue::Text(table), CborValue::Text(id)) = (&parts[0], &parts[1])
            {
                return Value::String(format!("{}:{}", table, id));
            }
            cbor_to_json(*inner)
        }
        CborValue::Tag(_, inner) => cbor_to_json(*inner),
        other => Value::String(format!("{:?}", other)),
    }
}

fn number_to_cbor(n: Number) -> CborValue {
    if let Some(i) = n.as_i64() {
        CborValue::Integer(Integer::from(i))
    } else if let Some(u) = n.as_u64() {
        CborValue::Integer(Integer::from(u))
    } else if let Some(f) = n.as_f64() {
        CborValue::Float(f)
    } else {
        CborValue::Null
    }
}

fn integer_to_json(i: Integer) -> Value {
    if let Ok(v) = i64::try_from(i) {
        return Value::Number(v.into());
    }
    if let Ok(v) = u64::try_from(i) {
        return Value::Number(v.into());
    }
    let raw: i128 = i.into();
    Number::from_f64(raw as f64).map(Value::Number).unwrap_or(Value::Null)
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
}
