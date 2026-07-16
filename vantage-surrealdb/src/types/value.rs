//! Value type implementations for SurrealDB
//!
//! AnySurrealType passthrough and conversions.

use crate::types::{AnySurrealType, SurrealType, SurrealTypeNoneMarker};
use base64::Engine as _;
use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_expressions::{Expression, Expressive};
use vantage_types::cbor_json::{CborDialect, cbor_to_json, json_to_cbor};

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

/// Rendering policy converting SurrealDB's tagged CBOR to JSON —
/// `Tag(8)` record ids, `Tag(12)` compact datetimes, etc. render the
/// same textual form SurrealDB's own JSON API would produce, instead of
/// falling through as their raw untagged payload.
///
/// A plain `ciborium::into_writer` + `ciborium::from_reader` round-trip
/// (the implementation before the shared walker) drops CBOR tags
/// entirely — a datetime (`Tag(12, [seconds, nanos])`) decoded to the
/// bare `[seconds, nanos]` array rather than a timestamp, and likewise
/// for every other tagged SurrealDB type.
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
            // Record id: Tag(8, [table, id]) -> "table:id".
            (8, CborValue::Array(parts))
                if matches!(parts.as_slice(), [CborValue::Text(_), CborValue::Text(_)]) =>
            {
                let [CborValue::Text(table), CborValue::Text(id)] = parts.as_slice() else {
                    unreachable!()
                };
                JsonValue::String(format!("{table}:{id}"))
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

/// `[seconds, nanos]` → RFC 3339 string. Falls back to the raw array (the
/// previous, broken behaviour) only if the shape or the timestamp itself is
/// invalid, so a malformed value is still visible rather than silently lost.
fn compact_datetime_to_json(inner: CborValue) -> JsonValue {
    let fallback = |inner| cbor_to_json(&SurrealDialect, inner);
    let CborValue::Array(parts) = &inner else {
        return fallback(inner);
    };
    let [CborValue::Integer(secs), CborValue::Integer(nanos)] = parts.as_slice() else {
        return fallback(inner);
    };
    let (Ok(secs), Ok(nanos)) = (i64::try_from(*secs), u32::try_from(*nanos)) else {
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
    let CborValue::Array(parts) = &inner else {
        return fallback(inner);
    };
    let [CborValue::Integer(secs), CborValue::Integer(nanos)] = parts.as_slice() else {
        return fallback(inner);
    };
    let (Ok(secs), Ok(nanos)) = (i64::try_from(*secs), u32::try_from(*nanos)) else {
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

/// Macro for explicit Expressive<AnySurrealType> impls on scalar types.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The shared walker under this crate's dialect, ref-shaped for the
    /// assertions below.
    fn cbor_to_json(v: &CborValue) -> JsonValue {
        super::cbor_to_json(&SurrealDialect, v.clone())
    }

    #[test]
    fn compact_datetime_becomes_rfc3339_not_a_raw_array() {
        // Tag(12, [seconds, nanos]) — SurrealDB's compact datetime encoding.
        // 1735689600 = 2025-01-01T00:00:00Z.
        let cbor = CborValue::Tag(
            12,
            Box::new(CborValue::Array(vec![
                CborValue::Integer(1_735_689_600i64.into()),
                CborValue::Integer(0i64.into()),
            ])),
        );
        let json = cbor_to_json(&cbor);
        assert_eq!(json, JsonValue::String("2025-01-01T00:00:00Z".to_string()));
    }

    #[test]
    fn compact_datetime_with_nanos() {
        let cbor = CborValue::Tag(
            12,
            Box::new(CborValue::Array(vec![
                CborValue::Integer(1_735_689_600i64.into()),
                CborValue::Integer(500_000_000i64.into()),
            ])),
        );
        let json = cbor_to_json(&cbor);
        assert_eq!(json.as_str().unwrap(), "2025-01-01T00:00:00.500Z");
    }

    #[test]
    fn rfc3339_datetime_tag_passes_through_as_string() {
        let cbor = CborValue::Tag(
            0,
            Box::new(CborValue::Text("2025-01-01T00:00:00Z".to_string())),
        );
        assert_eq!(
            cbor_to_json(&cbor),
            JsonValue::String("2025-01-01T00:00:00Z".to_string())
        );
    }

    #[test]
    fn record_id_becomes_table_colon_id() {
        let cbor = CborValue::Tag(
            8,
            Box::new(CborValue::Array(vec![
                CborValue::Text("login_audit".to_string()),
                CborValue::Text("abc123".to_string()),
            ])),
        );
        assert_eq!(
            cbor_to_json(&cbor),
            JsonValue::String("login_audit:abc123".to_string())
        );
    }

    #[test]
    fn compact_duration_becomes_iso8601() {
        let cbor = CborValue::Tag(
            14,
            Box::new(CborValue::Array(vec![
                CborValue::Integer(5400i64.into()), // 1h30m
                CborValue::Integer(0i64.into()),
            ])),
        );
        assert_eq!(
            cbor_to_json(&cbor),
            JsonValue::String("PT1H30M".to_string())
        );
    }

    #[test]
    fn any_surreal_type_json_conversion_uses_cbor_to_json() {
        let any = AnySurrealType::from_cbor(&CborValue::Tag(
            12,
            Box::new(CborValue::Array(vec![
                CborValue::Integer(1_735_689_600i64.into()),
                CborValue::Integer(0i64.into()),
            ])),
        ))
        .unwrap();
        let json: JsonValue = any.into();
        assert_eq!(json, JsonValue::String("2025-01-01T00:00:00Z".to_string()));
    }

    #[test]
    fn plain_values_still_convert_correctly() {
        assert_eq!(
            cbor_to_json(&CborValue::Integer(42i64.into())),
            JsonValue::from(42)
        );
        assert_eq!(
            cbor_to_json(&CborValue::Text("hi".into())),
            JsonValue::from("hi")
        );
        assert_eq!(cbor_to_json(&CborValue::Bool(true)), JsonValue::from(true));
        assert_eq!(cbor_to_json(&CborValue::Null), JsonValue::Null);
    }
}
