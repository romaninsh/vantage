//! Value coercion for CLI condition arguments.
//!
//! Three paths:
//! - **Column-typed** ([`coerce_for_column`]): the target column's
//!   declared type wins. `name=true` against a string column stays a
//!   string; `is_paying_client=true` against a bool column becomes a
//!   bool. Used by the Eq-condition wiring in `run` where the field
//!   name is known.
//! - **Auto-detect** ([`auto_detect`]): try integer → float → bool →
//!   text. The fallback when no column type is available (unknown
//!   columns, value-list elements for `:in=…`, narrowing on
//!   driver-returned id strings).
//! - **Typed JSON** (`#`-prefixed): forces the value to be parsed as a
//!   JSON literal, which preserves bool/int/float/null/array/map types
//!   unambiguously. Necessary when a string happens to look like a
//!   number or boolean, or when a heterogeneous list / nested map is
//!   needed.

use ciborium::Value as CborValue;
use vantage_core::{Context, Result, error};
use vantage_vista::Vista;

/// Coerce a raw CLI value string to CBOR. `#`-prefixed values are parsed
/// as JSON literals; everything else goes through the auto-detect path.
pub fn parse_value(s: &str) -> Result<CborValue> {
    if let Some(rest) = s.strip_prefix('#') {
        let json: serde_json::Value =
            serde_json::from_str(rest).context(error!("invalid JSON literal", literal = rest))?;
        Ok(json_to_cbor(json))
    } else {
        Ok(auto_detect(s))
    }
}

/// Column-typed coercion: `name=true` against a string column gives
/// `Text("true")`, not `Bool(true)`. `raw` is the user's text without a
/// `#` prefix; `#` values are parsed earlier.
///
/// Errors when `raw` doesn't parse as the column's declared type
/// (`calories=abc` on an `i64` column): a typo is better caught here than
/// sent to the driver as a filter that matches nothing.
///
/// Falls back to [`auto_detect`] (never errors) when the column is
/// missing, declares no type, or has an unrecognised type.
pub fn coerce_for_column(vista: &Vista, field: &str, raw: &str) -> Result<CborValue> {
    let Some(original_type) = vista
        .column(field)
        .and_then(|column| column.original_type.as_deref())
    else {
        return Ok(auto_detect(raw));
    };
    match column_kind(original_type) {
        ColumnKind::Bool => match raw {
            "true" => Ok(CborValue::Bool(true)),
            "false" => Ok(CborValue::Bool(false)),
            other => Err(error!(
                "bool column value is neither `true` nor `false`",
                field = field,
                value = other
            )),
        },
        ColumnKind::Int => raw
            .parse::<i64>()
            .map(|i| CborValue::Integer(i.into()))
            .map_err(|_| {
                error!(
                    "integer column value is not an integer",
                    field = field,
                    value = raw
                )
            }),
        ColumnKind::Float => raw.parse::<f64>().map(CborValue::Float).map_err(|_| {
            error!(
                "float column value is not a number",
                field = field,
                value = raw
            )
        }),
        ColumnKind::Text => Ok(CborValue::Text(raw.to_string())),
        ColumnKind::Unknown => Ok(auto_detect(raw)),
    }
}

enum ColumnKind {
    Bool,
    Int,
    Float,
    Text,
    Unknown,
}

/// The module path is ignored: `alloc::string::String` and `String` are both `Text`.
fn column_kind(original_type: &str) -> ColumnKind {
    let tail = original_type.rsplit("::").next().unwrap_or(original_type);
    match tail {
        "bool" | "boolean" => ColumnKind::Bool,
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" | "int" | "integer" => ColumnKind::Int,
        "f32" | "f64" | "float" | "double" => ColumnKind::Float,
        "String" | "str" | "string" | "text" => ColumnKind::Text,
        _ => ColumnKind::Unknown,
    }
}

/// Cheap heuristic typing for unprefixed values: integer if it parses,
/// then float, then `true`/`false`, else text. Drivers translate
/// further at their own boundary.
pub fn auto_detect(value: &str) -> CborValue {
    if let Ok(i) = value.parse::<i64>() {
        CborValue::Integer(i.into())
    } else if let Ok(f) = value.parse::<f64>() {
        CborValue::Float(f)
    } else if value == "true" {
        CborValue::Bool(true)
    } else if value == "false" {
        CborValue::Bool(false)
    } else {
        CborValue::Text(value.to_string())
    }
}

/// Translate a `serde_json::Value` into the matching `ciborium::Value`;
/// JSON integers become `Integer`, other numbers `Float`.
pub fn json_to_cbor(j: serde_json::Value) -> CborValue {
    vantage_types::cbor_json::json_to_cbor(j)
}

/// Parse a comma-separated value list for `field:in=a,b,c`. Each
/// element goes through `parse_value` so `#`-typed elements work
/// (`#1,#2,#"three"` etc., though `#[1,2,"three"]` is usually cleaner).
pub fn parse_value_list(s: &str) -> Result<Vec<CborValue>> {
    if let Some(rest) = s.strip_prefix('#') {
        let json: serde_json::Value =
            serde_json::from_str(rest).context(error!("invalid JSON array", literal = rest))?;
        match json {
            serde_json::Value::Array(arr) => Ok(arr.into_iter().map(json_to_cbor).collect()),
            other => Err(error!(
                "`:in=#…` expects a JSON array",
                got = other.to_string()
            )),
        }
    } else {
        Ok(s.split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(auto_detect)
            .collect())
    }
}
