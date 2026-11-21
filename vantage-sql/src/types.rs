//! Shared helpers: CBOR ↔ JSON bridge.
//!
//! CBOR→JSON is value-preserving: every CBOR value produces a JSON value that
//! retains the original data (possibly as a string when JSON has no matching
//! type). Tags are stripped, bytes become hex strings, NaN/Infinity become
//! string representations.
//!
//! JSON→CBOR is lossless (JSON is a subset of what CBOR can represent).

#[cfg(any(feature = "sqlite", feature = "mysql"))]
use std::borrow::Cow;

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

/// A `json` column's value: a map or an array, given as one or as the JSON
/// text a column stores. A scalar, or text that isn't a JSON object or
/// array, is no `json` value.
pub(crate) fn json_document(value: CborValue) -> Option<JsonValue> {
    let json = match value {
        CborValue::Text(text) => serde_json::from_str::<JsonValue>(&text).ok()?,
        value @ (CborValue::Map(_) | CborValue::Array(_)) => cbor_to_json(value),
        _ => return None,
    };
    (json.is_object() || json.is_array()).then_some(json)
}

/// What a text parameter binds for a value: its text, or a `json` map or
/// array as its JSON text. `Some(None)` binds NULL; `None` is a value no
/// text parameter takes.
#[cfg(any(feature = "sqlite", feature = "mysql"))]
pub(crate) fn text_param(cbor: &CborValue) -> Option<Option<Cow<'_, str>>> {
    Some(match cbor {
        CborValue::Null => None,
        CborValue::Text(s) => Some(Cow::Borrowed(s.as_str())),
        CborValue::Tag(_, inner) => match inner.as_ref() {
            CborValue::Text(s) => Some(Cow::Borrowed(s.as_str())),
            _ => return None,
        },
        CborValue::Map(_) | CborValue::Array(_) => {
            Some(Cow::Owned(cbor_to_json(cbor.clone()).to_string()))
        }
        _ => return None,
    })
}

/// The records a contained relation keeps in its host column's value: the
/// map (one record) or array of maps a `json` column holds, given as
/// itself or as its JSON text. `None` for no value.
pub(crate) fn contained_records<V>(
    host: &CborValue,
    wrap: impl Fn(CborValue) -> V,
) -> vantage_core::Result<Option<Vec<vantage_types::Record<V>>>> {
    let record =
        |pairs: Vec<(CborValue, CborValue)>| -> vantage_core::Result<vantage_types::Record<V>> {
            pairs
                .into_iter()
                .map(|(k, v)| match k {
                    CborValue::Text(k) => Ok((k, wrap(v))),
                    other => Err(vantage_core::error!(
                        "a contained record's field name is text",
                        key = format!("{other:?}")
                    )
                    .mark_invalid_value()),
                })
                .collect()
        };
    let refused = || {
        vantage_core::error!("a contained relation's host holds a JSON object or an array of them")
            .mark_invalid_value()
    };
    let value = match host {
        CborValue::Null => return Ok(None),
        CborValue::Text(text) if text.is_empty() => return Ok(None),
        CborValue::Text(_) => json_document(host.clone())
            .map(json_to_cbor)
            .ok_or_else(refused)?,
        other => other.clone(),
    };
    match value {
        CborValue::Map(pairs) => Ok(Some(vec![record(pairs)?])),
        CborValue::Array(items) => items
            .into_iter()
            .map(|item| match item {
                CborValue::Map(pairs) => record(pairs),
                _ => Err(refused()),
            })
            .collect::<vantage_core::Result<Vec<_>>>()
            .map(Some),
        _ => Err(refused()),
    }
}

/// The host column's value for `records`: one map, or an array of maps for a
/// `contains_many`; its `json` column type stores it.
pub(crate) fn contained_host<V>(
    records: Vec<vantage_types::Record<V>>,
    many: bool,
    unwrap: impl Fn(V) -> CborValue,
) -> CborValue {
    let mut maps: Vec<CborValue> = records
        .into_iter()
        .map(|record| {
            CborValue::Map(
                record
                    .into_iter()
                    .map(|(k, v)| (CborValue::Text(k), unwrap(v)))
                    .collect(),
            )
        })
        .collect();
    if many {
        CborValue::Array(maps)
    } else {
        maps.pop().unwrap_or(CborValue::Null)
    }
}

/// A stored cell of a column declared as the Rust type `declared`: JSON text
/// in a `json` column reads as the map or array it holds (`""` as no value);
/// a native `json`/`jsonb` cell is already one. Any other cell is returned
/// as it is.
pub(crate) fn read_json_cell(
    table: &str,
    column: &str,
    declared: &str,
    value: CborValue,
) -> vantage_core::Result<CborValue> {
    if declared != std::any::type_name::<JsonValue>() {
        return Ok(value);
    }
    let CborValue::Text(text) = &value else {
        return Ok(value);
    };
    if text.is_empty() {
        return Ok(CborValue::Null);
    }
    match json_document(value.clone()) {
        Some(json) => Ok(json_to_cbor(json)),
        // The stored text may be anything a writer left; the error names
        // its length, not its content.
        None => Err(vantage_core::error!(
            format!(
                "`{column}` of `{table}` holds {} characters of text that is not a JSON object or array",
                text.chars().count()
            ),
            table = table,
            column = column,
            expected = "json"
        )),
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
