//! Value type implementations for SurrealDB
//!
//! AnySurrealType passthrough and conversions.

use crate::types::{AnySurrealType, SurrealType, SurrealTypeNoneMarker};
use base64::Engine as _;
use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_expressions::{Expression, Expressive};

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
        let cbor = json_to_cbor(val);
        AnySurrealType::from_cbor(&cbor).expect("json_to_cbor produced unconvertible CBOR")
    }
}

impl From<AnySurrealType> for JsonValue {
    fn from(val: AnySurrealType) -> Self {
        cbor_to_json(val.value())
    }
}

/// Convert a CBOR value to JSON, understanding SurrealDB's tagged types
/// (`Tag(8)` record ids, `Tag(12)` compact datetimes, etc.) instead of
/// letting them fall through as their raw untagged payload.
///
/// A plain `ciborium::into_writer` + `ciborium::from_reader` round-trip
/// (the previous implementation) drops CBOR tags entirely — a datetime
/// (`Tag(12, [seconds, nanos])`) decoded to the bare `[seconds, nanos]`
/// array rather than a timestamp, and likewise for every other tagged
/// SurrealDB type. This walks the value directly so each tag renders the
/// same textual form SurrealDB's own JSON API would produce.
fn cbor_to_json(value: &CborValue) -> JsonValue {
    match value {
        CborValue::Null => JsonValue::Null,
        CborValue::Bool(b) => JsonValue::Bool(*b),
        CborValue::Integer(i) => integer_to_json(*i),
        CborValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        CborValue::Text(s) => JsonValue::String(s.clone()),
        CborValue::Bytes(b) => {
            JsonValue::String(base64::engine::general_purpose::STANDARD.encode(b))
        }
        CborValue::Array(arr) => JsonValue::Array(arr.iter().map(cbor_to_json).collect()),
        CborValue::Map(entries) => {
            let mut obj = serde_json::Map::with_capacity(entries.len());
            for (k, v) in entries {
                let key = match k {
                    CborValue::Text(s) => s.clone(),
                    other => format!("{other:?}"),
                };
                obj.insert(key, cbor_to_json(v));
            }
            JsonValue::Object(obj)
        }

        // RFC 3339 datetime, carried as text.
        CborValue::Tag(0, inner) => match inner.as_ref() {
            CborValue::Text(s) => JsonValue::String(s.clone()),
            other => cbor_to_json(other),
        },
        // NONE.
        CborValue::Tag(6, _) => JsonValue::Null,
        // Record id: Tag(8, [table, id]) -> "table:id".
        CborValue::Tag(8, inner) => match inner.as_ref() {
            CborValue::Array(parts) if parts.len() == 2 => match (&parts[0], &parts[1]) {
                (CborValue::Text(table), CborValue::Text(id)) => {
                    JsonValue::String(format!("{table}:{id}"))
                }
                _ => cbor_to_json(inner),
            },
            other => cbor_to_json(other),
        },
        // UUID, carried as text.
        CborValue::Tag(9, inner) => match inner.as_ref() {
            CborValue::Text(s) => JsonValue::String(s.clone()),
            other => cbor_to_json(other),
        },
        // Decimal, carried as text.
        CborValue::Tag(10, inner) => match inner.as_ref() {
            CborValue::Text(s) => JsonValue::String(s.clone()),
            other => cbor_to_json(other),
        },
        // Compact datetime: Tag(12, [seconds, nanos]) -> RFC 3339 string.
        CborValue::Tag(12, inner) => compact_datetime_to_json(inner),
        // Duration, carried as text.
        CborValue::Tag(13, inner) => match inner.as_ref() {
            CborValue::Text(s) => JsonValue::String(s.clone()),
            other => cbor_to_json(other),
        },
        // Compact duration: Tag(14, [seconds, nanos]) -> ISO 8601 duration.
        CborValue::Tag(14, inner) => compact_duration_to_json(inner),
        // UUID, carried as 16 raw bytes.
        CborValue::Tag(37, inner) => match inner.as_ref() {
            CborValue::Bytes(b) if b.len() == 16 => uuid::Uuid::from_slice(b)
                .map(|u| JsonValue::String(u.to_string()))
                .unwrap_or_else(|_| cbor_to_json(inner)),
            other => cbor_to_json(other),
        },

        // Unrecognised tag — recurse into the payload rather than dropping
        // it silently, matching AnySurrealType's Display fallback.
        CborValue::Tag(_, inner) => cbor_to_json(inner),

        _ => JsonValue::Null,
    }
}

fn integer_to_json(i: ciborium::value::Integer) -> JsonValue {
    if let Ok(v) = i64::try_from(i) {
        return JsonValue::Number(v.into());
    }
    if let Ok(v) = u64::try_from(i) {
        return JsonValue::Number(v.into());
    }
    let raw: i128 = i.into();
    serde_json::Number::from_f64(raw as f64)
        .map(JsonValue::Number)
        .unwrap_or(JsonValue::Null)
}

/// `[seconds, nanos]` → RFC 3339 string. Falls back to the raw array (the
/// previous, broken behaviour) only if the shape or the timestamp itself is
/// invalid, so a malformed value is still visible rather than silently lost.
fn compact_datetime_to_json(inner: &CborValue) -> JsonValue {
    let CborValue::Array(parts) = inner else {
        return cbor_to_json(inner);
    };
    let [CborValue::Integer(secs), CborValue::Integer(nanos)] = parts.as_slice() else {
        return cbor_to_json(inner);
    };
    let (Ok(secs), Ok(nanos)) = (i64::try_from(*secs), u32::try_from(*nanos)) else {
        return cbor_to_json(inner);
    };
    chrono::DateTime::from_timestamp(secs, nanos)
        .map(|dt| JsonValue::String(dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)))
        .unwrap_or_else(|| cbor_to_json(inner))
}

/// `[seconds, nanos]` → an ISO 8601 duration string (`PT1H30M`), falling
/// back to the raw array on an unexpected shape.
fn compact_duration_to_json(inner: &CborValue) -> JsonValue {
    let CborValue::Array(parts) = inner else {
        return cbor_to_json(inner);
    };
    let [CborValue::Integer(secs), CborValue::Integer(nanos)] = parts.as_slice() else {
        return cbor_to_json(inner);
    };
    let (Ok(secs), Ok(nanos)) = (i64::try_from(*secs), u32::try_from(*nanos)) else {
        return cbor_to_json(inner);
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

fn json_to_cbor(val: JsonValue) -> CborValue {
    match val {
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
        assert_eq!(cbor_to_json(&cbor), JsonValue::String("PT1H30M".to_string()));
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
        assert_eq!(cbor_to_json(&CborValue::Integer(42i64.into())), JsonValue::from(42));
        assert_eq!(cbor_to_json(&CborValue::Text("hi".into())), JsonValue::from("hi"));
        assert_eq!(cbor_to_json(&CborValue::Bool(true)), JsonValue::from(true));
        assert_eq!(cbor_to_json(&CborValue::Null), JsonValue::Null);
    }
}
