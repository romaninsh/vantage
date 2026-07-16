//! `AnyCmdType` — a presentation value type for rich CLI rendering, plus
//! the JSON ↔ CBOR helpers used across the crate.
//!
//! Internally the table flows `ciborium::Value` (the `TableSource::Value`
//! / `AnyType`), exactly like `vantage-aws`. `AnyCmdType` is only a
//! rendering convenience: built from a raw CBOR value plus the column's
//! declared Rust type so primitives parsed out of JSON render as
//! themselves. Mirrors the shape of `AnyAwsType`.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_types::{PlainDialect, RichText, Style, TerminalRender, cbor_json};

/// CBOR → JSON via the shared walker. Total — tagged values render their
/// payload instead of collapsing the whole value to `null` (the failure
/// mode of ciborium's serde bridge, which this used to route through).
pub(crate) fn cbor_to_json(v: &CborValue) -> JsonValue {
    cbor_json::cbor_to_json(&PlainDialect, v.clone())
}

/// JSON → CBOR. Total and lossless.
pub(crate) fn json_to_cbor(v: &JsonValue) -> CborValue {
    cbor_json::json_to_cbor(v.clone())
}

/// Render a CBOR scalar to a plain string (used for ids / aggregates).
pub(crate) fn cbor_to_string(v: &CborValue) -> String {
    cbor_json::cbor_to_string(&PlainDialect, v)
}

/// One value in a command-backed record, at the rendering boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum AnyCmdType {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Null,
    /// Lossless fallback for nested objects / arrays / byte strings.
    Other(CborValue),
}

impl AnyCmdType {
    /// Build a typed variant from a CBOR value given the column's declared
    /// Rust type name (from `ColumnLike::get_type`). CLI tools often emit
    /// numbers/bools as JSON strings; a declared primitive type coerces them.
    pub fn from_cbor_typed(value: CborValue, declared_type: &str) -> Self {
        if is_bool_type(declared_type)
            && let CborValue::Text(s) = &value
        {
            match s.as_str() {
                "true" => return AnyCmdType::Bool(true),
                "false" => return AnyCmdType::Bool(false),
                _ => {}
            }
        }
        if is_int_type(declared_type)
            && let CborValue::Text(s) = &value
            && let Ok(n) = s.parse::<i64>()
        {
            return AnyCmdType::Int(n);
        }
        if is_float_type(declared_type)
            && let CborValue::Text(s) = &value
            && let Ok(n) = s.parse::<f64>()
        {
            return AnyCmdType::Float(n);
        }
        Self::from_cbor_untyped(value)
    }

    /// Build a variant by inspecting the CBOR shape.
    pub fn from_cbor_untyped(value: CborValue) -> Self {
        match value {
            CborValue::Null => AnyCmdType::Null,
            CborValue::Bool(b) => AnyCmdType::Bool(b),
            CborValue::Text(s) => AnyCmdType::Text(s),
            CborValue::Integer(i) => {
                let n: i128 = i.into();
                match i64::try_from(n) {
                    Ok(n64) => AnyCmdType::Int(n64),
                    Err(_) => AnyCmdType::Other(CborValue::Integer(i)),
                }
            }
            CborValue::Float(f) => AnyCmdType::Float(f),
            CborValue::Tag(_, inner) => Self::from_cbor_untyped(*inner),
            other => AnyCmdType::Other(other),
        }
    }

    pub fn to_cbor(&self) -> CborValue {
        match self {
            AnyCmdType::Bool(b) => CborValue::Bool(*b),
            AnyCmdType::Int(i) => CborValue::Integer((*i).into()),
            AnyCmdType::Float(f) => CborValue::Float(*f),
            AnyCmdType::Text(s) => CborValue::Text(s.clone()),
            AnyCmdType::Null => CborValue::Null,
            AnyCmdType::Other(v) => v.clone(),
        }
    }
}

fn is_bool_type(name: &str) -> bool {
    name == "bool"
}
fn is_int_type(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
    )
}
fn is_float_type(name: &str) -> bool {
    matches!(name, "f32" | "f64")
}

impl TerminalRender for AnyCmdType {
    fn render(&self) -> RichText {
        match self {
            AnyCmdType::Bool(b) => b.render(),
            AnyCmdType::Int(i) => RichText::plain(i.to_string()),
            AnyCmdType::Float(f) => RichText::plain(f.to_string()),
            AnyCmdType::Text(s) => RichText::plain(s.clone()),
            AnyCmdType::Null => RichText::styled("—", Style::Muted),
            AnyCmdType::Other(v) => v.render(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_value_survives_json_conversion() {
        // The serde-bridge shortcut this replaced collapsed any tagged
        // value (and its whole containing map) to Null.
        let v = CborValue::Map(vec![(
            CborValue::Text("when".into()),
            CborValue::Tag(0, Box::new(CborValue::Text("2026-01-01T00:00:00Z".into()))),
        )]);
        assert_eq!(
            cbor_to_json(&v),
            serde_json::json!({"when": "2026-01-01T00:00:00Z"})
        );
    }
}
