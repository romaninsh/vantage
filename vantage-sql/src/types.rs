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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic round-trips ────────────────────────────────────────────────

    #[test]
    fn null_round_trips() {
        let cbor = CborValue::Null;
        let json = cbor_to_json(cbor);
        assert_eq!(json, JsonValue::Null);
        assert_eq!(json_to_cbor(json), CborValue::Null);
    }

    #[test]
    fn bool_round_trips() {
        let json = cbor_to_json(CborValue::Bool(true));
        assert_eq!(json, JsonValue::Bool(true));
        assert_eq!(json_to_cbor(json), CborValue::Bool(true));
    }

    #[test]
    fn integer_round_trips() {
        let json = cbor_to_json(CborValue::Integer(42.into()));
        assert_eq!(json, serde_json::json!(42));
        assert_eq!(json_to_cbor(json), CborValue::Integer(42.into()));
    }

    #[test]
    fn float_round_trips() {
        let json = cbor_to_json(CborValue::Float(1.337));
        assert_eq!(json, serde_json::json!(1.337));
        // JSON→CBOR for floats goes through as_f64
        assert_eq!(json_to_cbor(json), CborValue::Float(1.337));
    }

    #[test]
    fn text_round_trips() {
        let json = cbor_to_json(CborValue::Text("hello".into()));
        assert_eq!(json, serde_json::json!("hello"));
        assert_eq!(json_to_cbor(json), CborValue::Text("hello".into()));
    }

    // ── Value preservation (lossy but no data destroyed) ─────────────────

    #[test]
    fn nan_preserved_as_string() {
        let json = cbor_to_json(CborValue::Float(f64::NAN));
        assert_eq!(json, JsonValue::String("NaN".into()));
    }

    #[test]
    fn infinity_preserved_as_string() {
        let json = cbor_to_json(CborValue::Float(f64::INFINITY));
        assert_eq!(json, JsonValue::String("inf".into()));

        let json = cbor_to_json(CborValue::Float(f64::NEG_INFINITY));
        assert_eq!(json, JsonValue::String("-inf".into()));
    }

    #[test]
    fn u64_max_preserved_as_number() {
        // u64::MAX is beyond i64 but ciborium can represent it
        let cbor = CborValue::Integer(u64::MAX.into());
        let json = cbor_to_json(cbor);
        assert_eq!(json, serde_json::json!(u64::MAX));
    }

    #[test]
    fn bytes_valid_utf8_becomes_string() {
        let json = cbor_to_json(CborValue::Bytes(b"hello".to_vec()));
        assert_eq!(json, JsonValue::String("hello".into()));
    }

    #[test]
    fn bytes_invalid_utf8_becomes_hex() {
        let json = cbor_to_json(CborValue::Bytes(vec![0xFF, 0xFE]));
        assert_eq!(json, JsonValue::String("fffe".into()));
    }

    // ── Tag handling (tags stripped, values preserved) ────────────────────

    #[test]
    fn tag_datetime_stripped() {
        let cbor = CborValue::Tag(0, Box::new(CborValue::Text("2024-01-15T10:30:00Z".into())));
        let json = cbor_to_json(cbor);
        assert_eq!(json, JsonValue::String("2024-01-15T10:30:00Z".into()));
    }

    #[test]
    fn tag_decimal_integer_becomes_number() {
        let cbor = CborValue::Tag(10, Box::new(CborValue::Text("42".into())));
        let json = cbor_to_json(cbor);
        assert_eq!(json, serde_json::json!(42));
    }

    #[test]
    fn tag_decimal_safe_float_becomes_number() {
        // "3.5" round-trips through f64 cleanly
        let cbor = CborValue::Tag(10, Box::new(CborValue::Text("3.5".into())));
        let json = cbor_to_json(cbor);
        assert_eq!(json, serde_json::json!(3.5));
    }

    #[test]
    fn tag_decimal_high_precision_lossy() {
        // High-precision decimals lose precision through f64 — this is expected.
        // The JSON bridge is lossy by design; use Record<AnySqliteType> for lossless access.
        let s = "99999999999999999.123456789";
        let cbor = CborValue::Tag(10, Box::new(CborValue::Text(s.into())));
        let json = cbor_to_json(cbor);
        // f64 can't hold this precisely — becomes 1e17
        assert!(json.is_number());
    }

    #[test]
    fn tag_decimal_trailing_zeros_become_number() {
        // "1.10" → f64 1.1 — trailing zeros lost but value preserved
        let cbor = CborValue::Tag(10, Box::new(CborValue::Text("1.10".into())));
        let json = cbor_to_json(cbor);
        assert_eq!(json, serde_json::json!(1.1));
    }

    // ── Nested structures ────────────────────────────────────────────────

    #[test]
    fn array_round_trips() {
        let cbor = CborValue::Array(vec![
            CborValue::Integer(1.into()),
            CborValue::Text("two".into()),
        ]);
        let json = cbor_to_json(cbor);
        assert_eq!(json, serde_json::json!([1, "two"]));
    }

    #[test]
    fn map_with_text_keys_round_trips() {
        let cbor = CborValue::Map(vec![(
            CborValue::Text("key".into()),
            CborValue::Integer(99.into()),
        )]);
        let json = cbor_to_json(cbor);
        assert_eq!(json, serde_json::json!({"key": 99}));
    }

    #[test]
    fn map_non_text_key_uses_debug() {
        let cbor = CborValue::Map(vec![(
            CborValue::Integer(1.into()),
            CborValue::Text("val".into()),
        )]);
        let json = cbor_to_json(cbor);
        // Key becomes Debug representation
        assert!(
            json.as_object()
                .unwrap()
                .keys()
                .next()
                .unwrap()
                .contains("Integer")
        );
    }
}
