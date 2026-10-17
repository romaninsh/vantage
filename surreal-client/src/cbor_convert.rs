//! Lossy JSON↔CBOR transcoding behind `Engine::send_message`, the JSON
//! convenience path. Datetimes, durations, record ids and bytes are
//! best-effort here; `SurrealClient::query_cbor` keeps them exact.
//!
//! Rendering policy, matching SurrealDB's JSON output: bytes become base64,
//! record ids (`Tag(8)`) become `"table:id"`, other tags unwrap to their
//! payload, NaN/inf become `null`, and u64 values above `i64::MAX` survive.

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

    /// Integers outside `i64`/`u64` (CBOR negatives below `i64::MIN`) become
    /// f64, as in SurrealDB's own JSON output.
    fn big_int_to_json(&self, n: i128) -> Value {
        Number::from_f64(n as f64).map_or(Value::Null, Value::Number)
    }

    fn tag_to_json(&self, tag: u64, inner: CborValue) -> Value {
        // Tag(8, [table, id]) is a record id. Non-text ids stringify, so
        // `user:42` still renders as "user:42".
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
        cbor_json::cbor_to_json(self, inner)
    }
}

pub(crate) fn json_to_cbor(value: Value) -> CborValue {
    cbor_json::json_to_cbor(value)
}

pub(crate) fn cbor_to_json(value: CborValue) -> Value {
    cbor_json::cbor_to_json(&SurrealJsonDialect, value)
}
