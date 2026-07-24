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

/// Human-facing presentation of tagged CBOR, as used by grids, scripts
/// and MCP surfaces: SurrealDB record ids (`Tag(8)`) render as
/// `"table:id"`, `NONE` (`Tag(6)`) as `null`, datetimes / UUIDs /
/// decimals / durations (`Tag(0|9|10|13)`) keep their inner text, and
/// binary UUIDs (`Tag(37)`) render as hex. Everything else follows the
/// [`PlainDialect`] defaults.
#[derive(Debug, Clone, Copy, Default)]
pub struct PresentationDialect;

impl CborDialect for PresentationDialect {
    fn tag_to_json(&self, tag: u64, inner: CborValue) -> JsonValue {
        match (tag, inner) {
            // SurrealDB record id: Tag(8, [table, id]) -> "table:id",
            // stringifying a non-text id part (`user:42`).
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
            // SurrealDB NONE marker.
            (6, _) => JsonValue::Null,
            // RFC 3339 datetime (0), UUID (9), Decimal (10), Duration (13)
            // — all carry their displayable form as the inner text.
            (0 | 9 | 10 | 13, CborValue::Text(s)) => JsonValue::String(s),
            // SurrealDB datetime — `Tag(12, [seconds, nanos])`. Render as a
            // lossless nanosecond RFC-3339 string; `json_to_cbor_with_hint`
            // re-encodes it back to the same tag. Off-shape inners fall back
            // to the plain rendering.
            (12, inner) => tag12_to_rfc3339(&inner)
                .map(JsonValue::String)
                .unwrap_or_else(|| cbor_to_json(self, inner)),
            // UUID carried as raw bytes.
            (37, CborValue::Bytes(b)) => JsonValue::String(hex_encode(&b)),
            // Anything else: drop the tag, render the payload.
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
/// [`json_to_cbor`] is total but forward-only: it has no way to know that a
/// `"golf_course:pebble_beach"` string was once a SurrealDB record id, or
/// that a `"12"` string belongs in an integer column. That matters wherever
/// a value made a `CBOR → JSON → edit → JSON → CBOR` trip (a form field, an
/// MCP write): the naive back-conversion yields `Text("golf_course:…")`,
/// which never compares equal to the tracked `Tag(8, [table, id])`, so the
/// field reads as permanently changed.
///
/// The `hint` is the value the result is compared against (e.g. the record's
/// current CBOR). When it pins down a shape JSON erased, this restores it:
///
/// - **Record id** — a string under a `Tag(8, …)` hint becomes `Tag(8, …)`
///   again, mirroring the hint's inner shape (using the hint's own table to
///   locate the `:` split, so an id containing `:` survives).
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
        // Record id: restore Tag(8) from its "table:id" presentation form.
        (JsonValue::String(s), Some(CborValue::Tag(8, inner))) => retag_record_id(s, inner),
        // Epoch-pair datetime: re-encode the (edited) RFC-3339 string back
        // into `Tag(12, [seconds, nanos])`. An unparseable edit stays text —
        // the write path surfaces the type error rather than guessing.
        (JsonValue::String(s), Some(CborValue::Tag(12, _))) => {
            rfc3339_to_tag12(s).unwrap_or_else(|| CborValue::Text(s.clone()))
        }
        // Datetime / uuid / decimal / duration — the form edits the inner
        // text; re-wrap it in the same tag so it round-trips.
        (JsonValue::String(s), Some(CborValue::Tag(tag, inner)))
            if matches!(inner.as_ref(), CborValue::Text(_)) =>
        {
            CborValue::Tag(*tag, Box::new(CborValue::Text(s.clone())))
        }
        // Scalar coercions: a text field yields a string; land it as the
        // hinted scalar so an int / float / bool column round-trips.
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
        // A JSON number under a float hint stays a float — an integer-valued
        // number would otherwise narrow to Integer and drift from the hint.
        (JsonValue::Number(n), Some(CborValue::Float(_))) => {
            CborValue::Float(n.as_f64().unwrap_or(0.0))
        }
        // No hint, or a shape JSON already represents exactly: lossless path.
        _ => json_to_cbor(value.clone()),
    }
}

/// Rebuild a SurrealDB record id (`Tag(8)`) from its presentation string,
/// mirroring the `inner` shape carried by the hint.
fn retag_record_id(s: &str, inner: &CborValue) -> CborValue {
    let tag8 = |v: CborValue| CborValue::Tag(8, Box::new(v));
    match inner {
        // Tag(8, [Text(table), id]) — the canonical `table:id` shape. Split
        // on the hint's own table where possible so a `:` in the id part
        // survives; otherwise fall back to the first `:`.
        CborValue::Array(parts) if parts.len() == 2 => {
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
            // Preserve a non-text id part's scalar type where it round-trips.
            let id_val = match &parts[1] {
                CborValue::Integer(_) => id
                    .parse::<i64>()
                    .map(|i| CborValue::Integer(i.into()))
                    .unwrap_or(CborValue::Text(id)),
                _ => CborValue::Text(id),
            };
            tag8(CborValue::Array(vec![CborValue::Text(table), id_val]))
        }
        // Tag(8, Text("table:id")) — a single-text id form.
        _ => tag8(CborValue::Text(s.to_string())),
    }
}

/// Render SurrealDB's epoch-pair datetime — `Tag(12, [seconds, nanos])`
/// carries its payload as integers, not text — as a lossless RFC-3339
/// string (nanosecond fraction preserved, UTC). `None` when the inner
/// shape isn't the epoch pair.
pub fn tag12_to_rfc3339(inner: &CborValue) -> Option<String> {
    use chrono::{SecondsFormat, TimeZone as _};
    let CborValue::Array(arr) = inner else {
        return None;
    };
    let secs = cbor_i64(arr.first()?)?;
    let nanos = arr.get(1).and_then(cbor_i64).unwrap_or(0);
    let dt = chrono::Utc
        .timestamp_opt(secs, nanos.try_into().ok()?)
        .single()?;
    Some(dt.to_rfc3339_opts(SecondsFormat::Nanos, true))
}

/// Inverse of [`tag12_to_rfc3339`]: parse an (edited) RFC-3339 string back
/// into `Tag(12, [seconds, nanos])`, preserving the nanosecond fraction.
/// `None` when the string isn't RFC-3339 (the caller keeps it as `Text`).
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

fn cbor_i64(v: &CborValue) -> Option<i64> {
    match v {
        CborValue::Integer(i) => i64::try_from(i128::from(*i)).ok(),
        CborValue::Float(f) => Some(*f as i64),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scalars_convert_directly() {
        assert_eq!(
            cbor_to_json(&PlainDialect, CborValue::Null),
            JsonValue::Null
        );
        assert_eq!(
            cbor_to_json(&PlainDialect, CborValue::Bool(true)),
            json!(true)
        );
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
    fn tag12_renders_lossless_rfc3339_and_round_trips() {
        // 2027-07-22T09:30:00.123456789Z
        let tagged = CborValue::Tag(
            12,
            Box::new(CborValue::Array(vec![
                CborValue::Integer(1816421400.into()),
                CborValue::Integer(123_456_789.into()),
            ])),
        );
        let json = cbor_to_json(&PresentationDialect, tagged.clone());
        let JsonValue::String(s) = &json else {
            panic!("expected string, got {json:?}");
        };
        assert!(s.ends_with('Z'), "UTC preserved: {s}");
        assert!(s.contains(".123456789"), "nanos preserved: {s}");
        // The untouched round trip reproduces identical bytes — no
        // phantom dirty.
        assert_eq!(json_to_cbor_with_hint(&json, Some(&tagged)), tagged);
    }

    #[test]
    fn unparseable_edit_under_tag12_hint_stays_text() {
        let tagged = CborValue::Tag(
            12,
            Box::new(CborValue::Array(vec![
                CborValue::Integer(0.into()),
                CborValue::Integer(0.into()),
            ])),
        );
        assert_eq!(
            json_to_cbor_with_hint(&json!("not a date"), Some(&tagged)),
            CborValue::Text("not a date".into())
        );
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
    fn presentation_dialect_renders_surreal_tags() {
        let thing = CborValue::Tag(
            8,
            Box::new(CborValue::Array(vec![
                CborValue::Text("client".into()),
                CborValue::Text("abc".into()),
            ])),
        );
        assert_eq!(
            cbor_to_json(&PresentationDialect, thing),
            json!("client:abc")
        );
        let none = CborValue::Tag(6, Box::new(CborValue::Null));
        assert_eq!(cbor_to_json(&PresentationDialect, none), JsonValue::Null);
        let dec = CborValue::Tag(10, Box::new(CborValue::Text("12.34".into())));
        assert_eq!(cbor_to_json(&PresentationDialect, dec), json!("12.34"));
        let uuid = CborValue::Tag(37, Box::new(CborValue::Bytes(vec![0xab, 0xcd])));
        assert_eq!(cbor_to_json(&PresentationDialect, uuid), json!("abcd"));
        let numeric_id = CborValue::Tag(
            8,
            Box::new(CborValue::Array(vec![
                CborValue::Text("user".into()),
                CborValue::Integer(42.into()),
            ])),
        );
        assert_eq!(
            cbor_to_json(&PresentationDialect, numeric_id),
            json!("user:42")
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
        assert_eq!(cbor_to_string(&PlainDialect, &CborValue::Float(1.5)), "1.5");
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

    /// The presentation string of a `Tag(8)` record id, back-converted under
    /// that id as the hint, reconstructs the identical `Tag(8)` — so a form
    /// field over a record id compares equal to the tracked value.
    #[test]
    fn record_id_round_trips_under_hint() {
        let thing = CborValue::Tag(
            8,
            Box::new(CborValue::Array(vec![
                CborValue::Text("golf_course".into()),
                CborValue::Text("pebble_beach".into()),
            ])),
        );
        let rendered = cbor_to_json(&PresentationDialect, thing.clone());
        assert_eq!(rendered, json!("golf_course:pebble_beach"));
        assert_eq!(json_to_cbor_with_hint(&rendered, Some(&thing)), thing);
    }

    /// An id whose id-part itself contains `:` still splits on the hint's
    /// table boundary, not the first colon.
    #[test]
    fn record_id_with_colon_in_id_uses_table_boundary() {
        let thing = CborValue::Tag(
            8,
            Box::new(CborValue::Array(vec![
                CborValue::Text("event".into()),
                CborValue::Text("2026-07-23:slot".into()),
            ])),
        );
        let rendered = json!("event:2026-07-23:slot");
        assert_eq!(json_to_cbor_with_hint(&rendered, Some(&thing)), thing);
    }

    /// A numeric id part is restored as an integer, not stringified.
    #[test]
    fn record_id_preserves_numeric_id_part() {
        let thing = CborValue::Tag(
            8,
            Box::new(CborValue::Array(vec![
                CborValue::Text("user".into()),
                CborValue::Integer(42.into()),
            ])),
        );
        assert_eq!(
            json_to_cbor_with_hint(&json!("user:42"), Some(&thing)),
            thing
        );
    }

    /// A text-carrying tag (decimal here) re-wraps, and scalar string values
    /// coerce to the hinted int / float / bool.
    #[test]
    fn text_tag_and_scalars_coerce_under_hint() {
        let dec = CborValue::Tag(10, Box::new(CborValue::Text("12.34".into())));
        assert_eq!(json_to_cbor_with_hint(&json!("12.34"), Some(&dec)), dec);

        assert_eq!(
            json_to_cbor_with_hint(&json!("12"), Some(&CborValue::Integer(0.into()))),
            CborValue::Integer(12.into())
        );
        assert_eq!(
            json_to_cbor_with_hint(&json!("1.5"), Some(&CborValue::Float(0.0))),
            CborValue::Float(1.5)
        );
        assert_eq!(
            json_to_cbor_with_hint(&json!("true"), Some(&CborValue::Bool(false))),
            CborValue::Bool(true)
        );
        // A JSON integer-valued number under a float hint stays a float.
        assert_eq!(
            json_to_cbor_with_hint(&json!(3), Some(&CborValue::Float(0.0))),
            CborValue::Float(3.0)
        );
    }

    /// With no hint (or an unrelated one) it matches the lossless
    /// [`json_to_cbor`] exactly — a strict superset.
    #[test]
    fn no_hint_matches_lossless() {
        for v in [
            json!("s"),
            json!(7),
            json!(1.5),
            json!(true),
            json!(null),
            json!([1, 2]),
        ] {
            assert_eq!(json_to_cbor_with_hint(&v, None), json_to_cbor(v.clone()));
        }
        // An unparseable string under an integer hint falls back to text.
        assert_eq!(
            json_to_cbor_with_hint(&json!("abc"), Some(&CborValue::Integer(0.into()))),
            CborValue::Text("abc".into())
        );
    }
}
