//! `AnyAwsType` — the polymorphic value type held by AWS records.
//!
//! Mirrors the shape of `AnyCsvType` / `AnySurrealType`. Records flow
//! through the system as `Record<AnyAwsType>` so the rendering layer
//! can dispatch on a parsed variant (e.g. [`Arn`]) instead of a raw
//! [`ciborium::Value`]. The variant choice is driven by the column's
//! declared Rust type: `with_column_of::<Arn>(...)` becomes
//! `AnyAwsType::Arn(...)` here.

use ciborium::Value as CborValue;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use vantage_types::{RichText, Style, TerminalRender};

use super::arn::Arn;
use super::datetime::AwsDateTime;

/// One value in an AWS record. Variants carry parsed types when the
/// declared column type asked for them; otherwise we fall back to
/// representation-friendly variants ([`AnyAwsType::Text`],
/// [`AnyAwsType::Int`], …) and finally to the lossless
/// [`AnyAwsType::Other`] escape hatch.
#[derive(Debug, Clone, PartialEq)]
pub enum AnyAwsType {
    Arn(Arn),
    DateTime(AwsDateTime),
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Null,
    /// Lossless fallback for shapes we don't have a typed variant for
    /// (nested objects, byte strings, etc.).
    Other(CborValue),
}

impl AnyAwsType {
    /// Build a typed variant from a CBOR value, given the column's
    /// declared Rust type name (from [`vantage_table::traits::column_like::ColumnLike::get_type`]).
    ///
    /// The type name is matched against a small set of known suffixes —
    /// e.g. `"vantage_aws::types::arn::Arn"` ends in `"::arn::Arn"`,
    /// `"vantage_aws::types::datetime::AwsDateTime"` ends in
    /// `"::datetime::AwsDateTime"`. This avoids hard-coding full module
    /// paths that change with reorganisations.
    ///
    /// On parse failure of a domain type (e.g. malformed ARN string)
    /// we fall back to [`AnyAwsType::Text`] rather than panic, keeping
    /// the data visible.
    pub fn from_cbor_typed(value: CborValue, declared_type: &str) -> Self {
        if is_arn_type(declared_type)
            && let CborValue::Text(s) = &value
            && let Ok(arn) = Arn::try_parse(s)
        {
            return AnyAwsType::Arn(arn);
        }
        if is_datetime_type(declared_type)
            && let CborValue::Text(s) = &value
            && let Ok(dt) = AwsDateTime::try_parse(s)
        {
            return AnyAwsType::DateTime(dt);
        }
        // AWS Query (XML) returns booleans/numbers as text. If the
        // column declared a primitive type, parse the string into it
        // so rendering picks up the typed treatment.
        if is_bool_type(declared_type)
            && let CborValue::Text(s) = &value
        {
            match s.as_str() {
                "true" => return AnyAwsType::Bool(true),
                "false" => return AnyAwsType::Bool(false),
                _ => {}
            }
        }
        if is_int_type(declared_type)
            && let CborValue::Text(s) = &value
            && let Ok(n) = s.parse::<i64>()
        {
            return AnyAwsType::Int(n);
        }
        if is_float_type(declared_type)
            && let CborValue::Text(s) = &value
            && let Ok(n) = s.parse::<f64>()
        {
            return AnyAwsType::Float(n);
        }
        Self::from_cbor_untyped(value)
    }

    /// Build a variant by inspecting the CBOR shape, with light
    /// content sniffing for strings: anything starting with `"arn:"`
    /// is tried as an ARN; anything matching ISO 8601 / epoch
    /// timestamps is tried as an [`AwsDateTime`]. Used when the
    /// column has no domain-specific declared type (e.g. records
    /// returned via `AnyTable::list_values`).
    pub fn from_cbor_untyped(value: CborValue) -> Self {
        match value {
            CborValue::Null => AnyAwsType::Null,
            CborValue::Bool(b) => AnyAwsType::Bool(b),
            CborValue::Text(s) => {
                if s.starts_with("arn:") && let Ok(arn) = Arn::try_parse(&s) {
                    return AnyAwsType::Arn(arn);
                }
                if looks_like_iso_timestamp(&s) && let Ok(dt) = AwsDateTime::try_parse(&s) {
                    return AnyAwsType::DateTime(dt);
                }
                AnyAwsType::Text(s)
            }
            CborValue::Integer(i) => {
                let n: i128 = i.into();
                if let Ok(n64) = i64::try_from(n) {
                    AnyAwsType::Int(n64)
                } else {
                    AnyAwsType::Other(CborValue::Integer(i))
                }
            }
            CborValue::Float(f) => AnyAwsType::Float(f),
            CborValue::Tag(_, inner) => Self::from_cbor_untyped(*inner),
            other => AnyAwsType::Other(other),
        }
    }

    /// Borrow as a plain CBOR value when downstream code (conditions,
    /// expressions) needs the lossless representation.
    pub fn to_cbor(&self) -> CborValue {
        match self {
            AnyAwsType::Arn(a) => CborValue::Text(a.to_string()),
            AnyAwsType::DateTime(d) => CborValue::Text(d.to_string()),
            AnyAwsType::Bool(b) => CborValue::Bool(*b),
            AnyAwsType::Int(i) => CborValue::Integer((*i).into()),
            AnyAwsType::Float(f) => CborValue::Float(*f),
            AnyAwsType::Text(s) => CborValue::Text(s.clone()),
            AnyAwsType::Null => CborValue::Null,
            AnyAwsType::Other(v) => v.clone(),
        }
    }
}

fn is_arn_type(name: &str) -> bool {
    name.ends_with("::Arn") || name == "Arn"
}

fn is_datetime_type(name: &str) -> bool {
    name.ends_with("::AwsDateTime") || name == "AwsDateTime"
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

/// Cheap pre-filter for ISO 8601 timestamps before reaching for chrono.
/// Looks for `YYYY-MM-DDTHH:MM…` shape — enough to avoid running the
/// parser on every random string.
fn looks_like_iso_timestamp(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 16 {
        return false;
    }
    // `YYYY-MM-DDT…`
    bytes[..4].iter().all(|b| b.is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(|b| b.is_ascii_digit())
        && (bytes[10] == b'T' || bytes[10] == b' ')
        && bytes[11..13].iter().all(|b| b.is_ascii_digit())
        && bytes[13] == b':'
}

impl TerminalRender for AnyAwsType {
    fn render(&self) -> RichText {
        match self {
            AnyAwsType::Arn(a) => a.render(),
            AnyAwsType::DateTime(d) => d.render(),
            AnyAwsType::Bool(b) => b.render(),
            AnyAwsType::Int(i) => RichText::plain(i.to_string()),
            AnyAwsType::Float(f) => RichText::plain(f.to_string()),
            AnyAwsType::Text(s) => RichText::plain(s.clone()),
            AnyAwsType::Null => RichText::styled("—", Style::Muted),
            AnyAwsType::Other(v) => v.render(),
        }
    }
}

// Serde for AnyAwsType — preserves the typed variants by going through
// CBOR. Used by AssociatedExpression / `serde_json::Value` interop in
// downstream code that may serialise/deserialise records.
impl Serialize for AnyAwsType {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.to_cbor().serialize(ser)
    }
}

impl<'de> Deserialize<'de> for AnyAwsType {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let v = CborValue::deserialize(de)?;
        Ok(AnyAwsType::from_cbor_untyped(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arn_when_column_declared_as_arn() {
        let v = CborValue::Text("arn:aws:iam::123:user/alice".to_string());
        let t = AnyAwsType::from_cbor_typed(v, "vantage_aws::types::arn::Arn");
        matches!(t, AnyAwsType::Arn(_))
            .then_some(())
            .expect("expected Arn variant");
    }

    #[test]
    fn falls_back_to_text_when_column_is_string() {
        let v = CborValue::Text("hello".to_string());
        let t = AnyAwsType::from_cbor_typed(v, "alloc::string::String");
        matches!(t, AnyAwsType::Text(_))
            .then_some(())
            .expect("expected Text variant");
    }

    #[test]
    fn null_variant_renders_as_dash() {
        assert_eq!(AnyAwsType::Null.render().to_plain(), "—");
    }
}
