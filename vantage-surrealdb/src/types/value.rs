//! Value type implementations for SurrealDB
//!
//! AnySurrealType passthrough and conversions.

use crate::types::{AnySurrealType, SurrealType, SurrealTypeNoneMarker};
use base64::Engine as _;
use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_expressions::{Expression, Expressive};
use vantage_types::cbor_json::{CborDialect, cbor_to_json, cbor_to_string, json_to_cbor};

/// AnySurrealType implements SurrealType as a passthrough — it already holds
/// a ciborium::Value internally, so to_cbor/from_cbor just clone the inner value.
impl SurrealType for AnySurrealType {
    type Target = SurrealTypeNoneMarker;

    fn to_cbor(&self) -> CborValue {
        self.value().clone()
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        AnySurrealType::from_cbor(&cbor)
    }
}

// From impls for common types — enables `expr_param!` scalar conversion.
// Can't use blanket `From<T: SurrealType>` because it conflicts with std's `From<T> for T`.
macro_rules! impl_from_for_any {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for AnySurrealType {
                fn from(val: $ty) -> Self {
                    AnySurrealType::new(val)
                }
            }
        )*
    };
}

impl_from_for_any!(
    i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64, bool, String
);

impl From<&str> for AnySurrealType {
    fn from(val: &str) -> Self {
        AnySurrealType::new(val.to_string())
    }
}

impl From<JsonValue> for AnySurrealType {
    fn from(val: JsonValue) -> Self {
        // json_to_cbor is total, and From<CborValue> classifies without
        // failing — no panic path.
        AnySurrealType::from(json_to_cbor(val))
    }
}

impl From<AnySurrealType> for JsonValue {
    fn from(val: AnySurrealType) -> Self {
        cbor_to_json(&SurrealDialect, val.into_value())
    }
}

/// Converts SurrealDB's tagged CBOR to JSON: `Tag(8)` record ids, `Tag(12)`
/// compact datetimes and the other tagged types render the text SurrealDB's
/// own JSON API produces, not their raw payload.
pub struct SurrealDialect;

impl CborDialect for SurrealDialect {
    fn bytes_to_json(&self, bytes: Vec<u8>) -> JsonValue {
        JsonValue::String(base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    /// Integers beyond `u64` (only negatives below `i64::MIN` in CBOR):
    /// best-effort f64, matching SurrealDB's own JSON output.
    fn big_int_to_json(&self, n: i128) -> JsonValue {
        serde_json::Number::from_f64(n as f64)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null)
    }

    fn tag_to_json(&self, tag: u64, inner: CborValue) -> JsonValue {
        match (tag, inner) {
            // RFC 3339 datetime (0), UUID (9), Decimal (10) and
            // Duration (13), all carried as their displayable text.
            (0 | 9 | 10 | 13, CborValue::Text(s)) => JsonValue::String(s),
            // NONE.
            (6, _) => JsonValue::Null,
            // Record id: Tag(8, [table, id]) -> "table:id". Non-text id
            // parts (numeric/compound record ids) stringify, so `user:42`
            // still renders canonically.
            (8, CborValue::Array(parts)) if matches!(parts.as_slice(), [CborValue::Text(_), _]) => {
                let mut it = parts.into_iter();
                let (table, id) = (it.next().expect("len 2"), it.next().expect("len 2"));
                let (CborValue::Text(table), id) = (table, id) else {
                    unreachable!()
                };
                match id {
                    CborValue::Text(id) => JsonValue::String(format!("{table}:{id}")),
                    other => JsonValue::String(format!("{table}:{}", cbor_to_string(self, &other))),
                }
            }
            // Compact datetime: Tag(12, [seconds, nanos]) -> RFC 3339 string.
            (12, inner) => compact_datetime_to_json(inner),
            // Compact duration: Tag(14, [seconds, nanos]) -> ISO 8601 duration.
            (14, inner) => compact_duration_to_json(inner),
            // UUID, carried as 16 raw bytes.
            (37, CborValue::Bytes(b)) if b.len() == 16 => uuid::Uuid::from_slice(&b)
                .map(|u| JsonValue::String(u.to_string()))
                .unwrap_or_else(|_| self.bytes_to_json(b)),
            // Unrecognised tag or unexpected payload shape — recurse into
            // the payload rather than dropping it silently, matching
            // AnySurrealType's Display fallback.
            (_, inner) => cbor_to_json(self, inner),
        }
    }
}

/// Splits a compact `[seconds, nanos]` payload. SurrealDB omits trailing zero
/// parts, so `[]`, `[seconds]` and `[seconds, nanos]` are all valid.
fn compact_secs_nanos(inner: &CborValue) -> Option<(i64, u32)> {
    let CborValue::Array(parts) = inner else {
        return None;
    };
    let part = |value: Option<&CborValue>| match value {
        None => Some(0i128),
        Some(CborValue::Integer(n)) => Some(i128::from(*n)),
        Some(_) => None,
    };
    if parts.len() > 2 {
        return None;
    }
    let secs = i64::try_from(part(parts.first())?).ok()?;
    let nanos = u32::try_from(part(parts.get(1))?).ok()?;
    Some((secs, nanos))
}

/// `[seconds, nanos]` → RFC 3339 string. A malformed shape or timestamp
/// falls back to the raw array, so it stays visible rather than lost.
fn compact_datetime_to_json(inner: CborValue) -> JsonValue {
    let fallback = |inner| cbor_to_json(&SurrealDialect, inner);
    let Some((secs, nanos)) = compact_secs_nanos(&inner) else {
        return fallback(inner);
    };
    chrono::DateTime::from_timestamp(secs, nanos)
        .map(|dt| JsonValue::String(dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)))
        .unwrap_or_else(|| fallback(inner))
}

/// `[seconds, nanos]` → an ISO 8601 duration string (`PT1H30M`), falling
/// back to the raw array on an unexpected shape.
fn compact_duration_to_json(inner: CborValue) -> JsonValue {
    let fallback = |inner| cbor_to_json(&SurrealDialect, inner);
    let Some((secs, nanos)) = compact_secs_nanos(&inner) else {
        return fallback(inner);
    };
    let mut out = String::from("PT");
    let (h, rem) = (secs / 3600, secs % 3600);
    let (m, s) = (rem / 60, rem % 60);
    if h != 0 {
        out.push_str(&format!("{h}H"));
    }
    if m != 0 {
        out.push_str(&format!("{m}M"));
    }
    if s != 0 || nanos != 0 || (h == 0 && m == 0) {
        if nanos == 0 {
            out.push_str(&format!("{s}S"));
        } else {
            out.push_str(&format!("{s}.{nanos:09}S"));
        }
    }
    JsonValue::String(out)
}

/// Macro for explicit `Expressive<AnySurrealType>` impls on scalar types.
/// Lets you pass `25i64`, `true`, `"hello"` directly to RefOperation methods.
macro_rules! impl_expressive_for_scalar {
    ($($ty:ty),*) => {
        $(
            impl Expressive<AnySurrealType> for $ty {
                fn expr(&self) -> Expression<AnySurrealType> {
                    Expression::new(
                        "{}",
                        vec![vantage_expressions::ExpressiveEnum::Scalar(
                            AnySurrealType::new_ref(self),
                        )],
                    )
                }
            }
        )*
    };
}

impl_expressive_for_scalar!(
    i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64, bool, String
);

impl Expressive<AnySurrealType> for &str {
    fn expr(&self) -> Expression<AnySurrealType> {
        Expression::new(
            "{}",
            vec![vantage_expressions::ExpressiveEnum::Scalar(
                AnySurrealType::from(self.to_string()),
            )],
        )
    }
}

impl Expressive<AnySurrealType> for AnySurrealType {
    fn expr(&self) -> Expression<AnySurrealType> {
        Expression::new(
            "{}",
            vec![vantage_expressions::ExpressiveEnum::Scalar(self.clone())],
        )
    }
}
