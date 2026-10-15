//! Shared helpers: CBOR ↔ JSON bridge.
//!
//! CBOR→JSON is value-preserving: every CBOR value produces a JSON value that
//! retains the original data (possibly as a string when JSON has no matching
//! type). Tags are stripped, bytes become hex strings, NaN/Infinity become
//! string representations.
//!
//! JSON→CBOR is lossless (JSON is a subset of what CBOR can represent).

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_types::cbor_json::{self, CborDialect};

/// Parse a contained relation's host-column value into a CBOR map/array.
/// A SQL `TEXT` column holding JSON comes back as `CborValue::Text` (parse it);
/// a native `json`/`jsonb` column already decodes to `Map`/`Array` (pass
/// through). Returns `None` for anything else (e.g. NULL/absent column).
#[cfg(feature = "vista")]
pub(crate) fn parse_json_host(v: &CborValue) -> Option<CborValue> {
    match v {
        CborValue::Text(s) => serde_json::from_str::<JsonValue>(s).ok().map(json_to_cbor),
        CborValue::Map(_) | CborValue::Array(_) => Some(v.clone()),
        _ => None,
    }
}

/// Rendering policy for the SQL drivers' JSON bridge.
///
/// Value-preserving rather than shape-preserving: every CBOR value
/// produces a JSON value that retains the original data (possibly as a
/// string when JSON has no matching type). NaN/Infinity become string
/// representations, bytes become UTF-8 or hex strings, and decimals are
/// converted to f64 (losing trailing zeros and precision beyond ~15
/// digits).
pub(crate) struct SqlDialect;

impl CborDialect for SqlDialect {
    /// NaN and Infinity have no JSON representation — preserve as string.
    fn float_to_json(&self, f: f64) -> JsonValue {
        serde_json::Number::from_f64(f)
            .map(JsonValue::Number)
            .unwrap_or_else(|| JsonValue::String(f.to_string()))
    }

    /// Bytes that are valid UTF-8 pass through as text; the rest as hex.
    fn bytes_to_json(&self, bytes: Vec<u8>) -> JsonValue {
        match String::from_utf8(bytes) {
            Ok(s) => JsonValue::String(s),
            Err(e) => JsonValue::String(cbor_json::hex_encode(e.as_bytes())),
        }
    }

    fn map_key_to_string(&self, key: CborValue) -> String {
        match key {
            CborValue::Text(s) => s,
            other => format!("{:?}", other),
        }
    }

    fn tag_to_json(&self, tag: u64, inner: CborValue) -> JsonValue {
        match (tag, inner) {
            // Decimal — try to produce a JSON number, fall back to string.
            // JSON bridge is lossy by design; trailing zeros and high
            // precision may be lost but the numeric value is preserved
            // when f64 suffices.
            (10, CborValue::Text(s)) => {
                if let Ok(i) = s.parse::<i64>() {
                    JsonValue::Number(i.into())
                } else if let Ok(f) = s.parse::<f64>() {
                    serde_json::Number::from_f64(f)
                        .map(JsonValue::Number)
                        .unwrap_or(JsonValue::String(s))
                } else {
                    JsonValue::String(s)
                }
            }
            // Other tags are stripped; the payload renders as-is.
            (_, inner) => cbor_json::cbor_to_json(self, inner),
        }
    }
}

/// Convert a `serde_json::Value` into a `ciborium::Value`. Lossless.
pub(crate) fn json_to_cbor(val: JsonValue) -> CborValue {
    cbor_json::json_to_cbor(val)
}

/// Convert a `ciborium::Value` into a `serde_json::Value` under
/// [`SqlDialect`].
pub(crate) fn cbor_to_json(val: CborValue) -> JsonValue {
    cbor_json::cbor_to_json(&SqlDialect, val)
}
