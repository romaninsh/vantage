//! CBOR to JSON rendering shared by every crate that shows `ciborium::Value`
//! as `serde_json::Value`.
//!
//! The value tree is walked directly because ciborium's serde bridge fails
//! on any tagged value. What JSON cannot express (tags, bytes, big integers,
//! non-finite floats, non-text keys) is rendered by a [`CborDialect`], so each
//! backend overrides only the hooks where it differs. JSON to CBOR needs no
//! dialect: [`json_to_cbor`] is total and lossless.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;

use crate::Record;

/// Rendering policy for CBOR shapes JSON can't represent directly.
///
/// Every method has a neutral default; recursion happens by calling back
/// into [`cbor_to_json`] with the same dialect.
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

    /// An integer outside both `i64` and `u64`. Default: decimal string, so
    /// the digits survive.
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

/// Human-facing presentation of tagged CBOR, as used by grids, scripts
/// and MCP surfaces: SurrealDB record ids (`Tag(8)`) render as
/// `"table:id"`, `NONE` (`Tag(6)`) as `null`, datetimes / UUIDs /
/// decimals / durations (`Tag(0|9|10|13)`) keep their inner text, epoch-pair
/// datetimes (`Tag(12)`) render as RFC 3339, and binary UUIDs (`Tag(37)`)
/// render as hex. Everything else follows the [`PlainDialect`] defaults.
#[derive(Debug, Clone, Copy, Default)]
pub struct PresentationDialect;

impl CborDialect for PresentationDialect {
    fn tag_to_json(&self, tag: u64, inner: CborValue) -> JsonValue {
        match (tag, inner) {
            (8, CborValue::Array(parts)) if parts.len() == 2 => {
                let mut it = parts.into_iter();
                let (table, id) = (it.next().expect("len 2"), it.next().expect("len 2"));
                match (table, id) {
                    (CborValue::Text(t), CborValue::Text(i)) => {
                        JsonValue::String(format!("{t}:{i}"))
                    }
                    (CborValue::Text(t), other) => {
                        JsonValue::String(format!("{t}:{}", cbor_to_string(self, &other)))
                    }
                    (a, b) => JsonValue::Array(vec![cbor_to_json(self, a), cbor_to_json(self, b)]),
                }
            }
            (6, _) => JsonValue::Null,
            (0 | 9 | 10 | 13, CborValue::Text(s)) => JsonValue::String(s),
            // Off-shape inners fall back to the plain rendering.
            (12, inner) => tag12_to_rfc3339(&inner)
                .map(JsonValue::String)
                .unwrap_or_else(|| cbor_to_json(self, inner)),
            (37, CborValue::Bytes(b)) => JsonValue::String(hex_encode(&b)),
            (_, inner) => cbor_to_json(self, inner),
        }
    }
}

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
        CborValue::Array(arr) => {
            JsonValue::Array(arr.into_iter().map(|v| cbor_to_json(dialect, v)).collect())
        }
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
        JsonValue::Array(arr) => CborValue::Array(arr.into_iter().map(json_to_cbor).collect()),
        JsonValue::Object(map) => CborValue::Map(
            map.into_iter()
                .map(|(k, v)| (CborValue::Text(k), json_to_cbor(v)))
                .collect(),
        ),
    }
}

/// Convert a JSON value to CBOR, using an existing CBOR value as a *shape
/// hint* so that lossy [`PresentationDialect`] renderings round-trip.
///
/// [`json_to_cbor`] is forward-only: it cannot know that a
/// `"golf_course:pebble_beach"` string was once a SurrealDB record id, or
/// that a `"12"` string belongs in an integer column. Without the hint a
/// value that made a `CBOR → JSON → edit → JSON → CBOR` trip never compares
/// equal to the tracked original, so the field reads as permanently changed.
///
/// The `hint` is the value the result is compared against (e.g. the record's
/// current CBOR). When it pins down a shape JSON erased, this restores it:
///
/// - **Record id** — a string under a `Tag(8, …)` hint becomes `Tag(8, …)`
///   again, mirroring the hint's inner shape (using the hint's own table to
///   locate the `:` split, so an id containing `:` survives).
/// - **Epoch-pair datetime** — a string under a `Tag(12, …)` hint equal to the
///   hint's own rendering reproduces the hint exactly; an edited RFC 3339
///   string re-encodes as `Tag(12, [seconds, nanos])`; an unparseable edit
///   stays text.
/// - **Text-carrying tag** — a string under a `Tag(t, Text(_))` hint
///   (datetime `0`, uuid `9`, decimal `10`, duration `13`) re-wraps as
///   `Tag(t, Text(s))`.
/// - **Scalar** — a string under an `Integer` / `Float` / `Bool` hint parses
///   to that scalar (form fields carry every value as text); a JSON number
///   under a `Float` hint stays a float instead of narrowing to an integer.
///
/// Anything the hint doesn't cover falls back to [`json_to_cbor`], so this is
/// a strict superset — safe to call with `hint: None`.
pub fn json_to_cbor_with_hint(value: &JsonValue, hint: Option<&CborValue>) -> CborValue {
    match (value, hint) {
        (JsonValue::String(s), Some(CborValue::Tag(8, inner))) => retag_record_id(s, inner),
        (JsonValue::String(s), Some(CborValue::Tag(12, inner))) => {
            // Untouched round trip keeps the hint's shape (a one-element
            // `[seconds]` stays one element) instead of normalising it.
            if tag12_to_rfc3339(inner).as_deref() == Some(s.as_str()) {
                CborValue::Tag(12, inner.clone())
            } else {
                rfc3339_to_tag12(s).unwrap_or_else(|| CborValue::Text(s.clone()))
            }
        }
        (JsonValue::String(s), Some(CborValue::Tag(tag, inner)))
            if matches!(inner.as_ref(), CborValue::Text(_)) =>
        {
            CborValue::Tag(*tag, Box::new(CborValue::Text(s.clone())))
        }
        (JsonValue::String(s), Some(CborValue::Integer(_))) => s
            .parse::<i64>()
            .map(|i| CborValue::Integer(i.into()))
            .unwrap_or_else(|_| CborValue::Text(s.clone())),
        (JsonValue::String(s), Some(CborValue::Float(_))) => s
            .parse::<f64>()
            .map(CborValue::Float)
            .unwrap_or_else(|_| CborValue::Text(s.clone())),
        (JsonValue::String(s), Some(CborValue::Bool(_)))
            if matches!(s.as_str(), "true" | "false") =>
        {
            CborValue::Bool(s == "true")
        }
        // An integer-valued number would otherwise narrow to Integer and
        // drift from the hint.
        (JsonValue::Number(n), Some(CborValue::Float(_))) => {
            CborValue::Float(n.as_f64().unwrap_or(0.0))
        }
        _ => json_to_cbor(value.clone()),
    }
}

/// Rebuild a SurrealDB record id (`Tag(8)`) from its presentation string,
/// mirroring the `inner` shape carried by the hint.
fn retag_record_id(s: &str, inner: &CborValue) -> CborValue {
    let tag8 = |v: CborValue| CborValue::Tag(8, Box::new(v));
    match inner {
        CborValue::Array(parts) if parts.len() == 2 => {
            // Split on the hint's own table where possible so a `:` in the
            // id part survives; otherwise fall back to the first `:`.
            let (table, id) = match &parts[0] {
                CborValue::Text(t)
                    if s.strip_prefix(t.as_str())
                        .and_then(|r| r.strip_prefix(':'))
                        .is_some() =>
                {
                    (t.clone(), s[t.len() + 1..].to_string())
                }
                _ => match s.split_once(':') {
                    Some((t, i)) => (t.to_string(), i.to_string()),
                    None => (String::new(), s.to_string()),
                },
            };
            let id_val = match &parts[1] {
                CborValue::Integer(_) => id
                    .parse::<i64>()
                    .map(|i| CborValue::Integer(i.into()))
                    .unwrap_or(CborValue::Text(id)),
                _ => CborValue::Text(id),
            };
            tag8(CborValue::Array(vec![CborValue::Text(table), id_val]))
        }
        _ => tag8(CborValue::Text(s.to_string())),
    }
}

/// Render SurrealDB's epoch-pair datetime — `Tag(12, [seconds, nanos])` —
/// as a lossless RFC-3339 string (nanosecond fraction preserved, UTC).
/// `None` when the inner shape isn't the epoch pair: strictly one or two
/// **integer** elements (a one-element form means zero nanos).
pub fn tag12_to_rfc3339(inner: &CborValue) -> Option<String> {
    use chrono::{SecondsFormat, TimeZone as _};
    let CborValue::Array(arr) = inner else {
        return None;
    };
    if arr.is_empty() || arr.len() > 2 {
        return None;
    }
    let secs = cbor_i64(&arr[0])?;
    let nanos = match arr.get(1) {
        Some(v) => cbor_i64(v)?,
        None => 0,
    };
    let dt = chrono::Utc
        .timestamp_opt(secs, nanos.try_into().ok()?)
        .single()?;
    Some(dt.to_rfc3339_opts(SecondsFormat::Nanos, true))
}

/// Inverse of [`tag12_to_rfc3339`]: parse an RFC-3339 string back into
/// `Tag(12, [seconds, nanos])`. `None` when the string isn't RFC-3339.
pub fn rfc3339_to_tag12(s: &str) -> Option<CborValue> {
    let dt = chrono::DateTime::parse_from_rfc3339(s)
        .ok()?
        .with_timezone(&chrono::Utc);
    Some(CborValue::Tag(
        12,
        Box::new(CborValue::Array(vec![
            CborValue::Integer(dt.timestamp().into()),
            CborValue::Integer(i64::from(dt.timestamp_subsec_nanos()).into()),
        ])),
    ))
}

// Integer-only on purpose: a float in an epoch pair is malformed and
// must fall back to the plain rendering, not get truncated.
fn cbor_i64(v: &CborValue) -> Option<i64> {
    match v {
        CborValue::Integer(i) => i64::try_from(i128::from(*i)).ok(),
        _ => None,
    }
}

/// Stringify a CBOR value for use as a **record id** — a cache key, a
/// traversal target, a selection identity.
///
/// Not interchangeable with [`cbor_to_string`]: a shape that is not a
/// plausible id (`Null`, `Float`, `Bytes`, `Array`, `Map`) yields `None` so
/// the caller can skip the row, where `cbor_to_string` would hand back `""`
/// for `Null` and key a cache entry under "no id at all".
///
/// A `Tag(8, [table, id])` renders `table:id`. Any other tag is transparent:
/// the id is whatever the payload stringifies to.
pub fn cbor_id_to_string(value: &CborValue) -> Option<String> {
    match value {
        CborValue::Text(s) => Some(s.clone()),
        CborValue::Integer(i) => Some(i128::from(*i).to_string()),
        CborValue::Bool(b) => Some(b.to_string()),
        CborValue::Tag(8, inner) => match inner.as_ref() {
            CborValue::Array(arr) if arr.len() == 2 => {
                let table = cbor_id_to_string(&arr[0])?;
                let id = cbor_id_to_string(&arr[1])?;
                Some(format!("{table}:{id}"))
            }
            other => cbor_id_to_string(other),
        },
        CborValue::Tag(_, inner) => cbor_id_to_string(inner),
        _ => None,
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
