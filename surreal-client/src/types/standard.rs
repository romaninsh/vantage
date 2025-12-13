//! Standard Rust type implementations for SurrealType trait using vantage-types

use super::{
    SurrealType, SurrealTypeAnyMarker, SurrealTypeBoolMarker, SurrealTypeDateTimeMarker,
    SurrealTypeDurationMarker, SurrealTypeFloatMarker, SurrealTypeIntMarker,
    SurrealTypeStringMarker,
};
use ciborium::value::Value as CborValue;

/// Represents an untyped/null SurrealDB value
#[derive(Debug, Clone)]
pub struct Any;

impl SurrealType for Any {
    type Target = SurrealTypeAnyMarker;

    fn to_cbor(&self) -> CborValue {
        // Tag 6: NONE value
        CborValue::Tag(6, Box::new(CborValue::Null))
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Tag(6, _) => Some(Any),
            CborValue::Null => Some(Any),
            _ => None,
        }
    }
}

// DateTime implementations
impl SurrealType for chrono::DateTime<chrono::Utc> {
    type Target = SurrealTypeDateTimeMarker;

    fn to_cbor(&self) -> CborValue {
        let timestamp = self.timestamp();
        let nanos = self.timestamp_subsec_nanos();
        CborValue::Tag(
            12,
            Box::new(CborValue::Array(vec![
                CborValue::Integer(timestamp.into()),
                CborValue::Integer(nanos.into()),
            ])),
        )
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Tag(12, boxed_value) => {
                if let CborValue::Array(arr) = boxed_value.as_ref()
                    && arr.len() == 2
                    && let (CborValue::Integer(secs), CborValue::Integer(nanos)) =
                        (&arr[0], &arr[1])
                {
                    let seconds = i128::from(*secs) as i64;
                    let nanos = i128::from(*nanos) as u32;
                    // Use chrono's timestamp_opt which handles negative timestamps properly
                    if let Some(dt) = chrono::DateTime::from_timestamp(seconds, nanos) {
                        return Some(dt);
                    }
                }
                None
            }
            _ => None,
        }
    }
}

impl SurrealType for std::time::SystemTime {
    type Target = SurrealTypeDateTimeMarker;

    fn to_cbor(&self) -> CborValue {
        let datetime: chrono::DateTime<chrono::Utc> = (*self).into();
        let timestamp = datetime.timestamp();
        let nanos = datetime.timestamp_subsec_nanos();
        CborValue::Tag(
            12,
            Box::new(CborValue::Array(vec![
                CborValue::Integer(timestamp.into()),
                CborValue::Integer(nanos.into()),
            ])),
        )
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Tag(12, boxed_value) => {
                if let CborValue::Array(arr) = boxed_value.as_ref()
                    && arr.len() == 2
                    && let (CborValue::Integer(secs), CborValue::Integer(nanos)) =
                        (&arr[0], &arr[1])
                {
                    let seconds = i128::from(*secs) as i64;
                    let nanos = i128::from(*nanos) as u32;
                    // Use chrono's timestamp which handles negative timestamps properly
                    if let Some(dt) = chrono::DateTime::from_timestamp(seconds, nanos) {
                        return Some(dt.into());
                    }
                }
                None
            }
            _ => None,
        }
    }
}

// String types
impl SurrealType for String {
    type Target = SurrealTypeStringMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Text(self.clone())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Text(s) => Some(s),
            _ => None,
        }
    }
}

impl SurrealType for &'static str {
    type Target = SurrealTypeStringMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Text(self.to_string())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        // Note: This is problematic for static str - we can't reconstruct static references
        // from CBOR. This is mainly for compatibility.
        match cbor {
            CborValue::Text(_) => None, // Can't convert back to &'static str
            _ => None,
        }
    }
}

// Integer types - all map to Int
impl SurrealType for i8 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => {
                let val = i128::from(i);
                if val >= i8::MIN as i128 && val <= i8::MAX as i128 {
                    Some(val as i8)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl SurrealType for i16 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => {
                let val = i128::from(i);
                if val >= i16::MIN as i128 && val <= i16::MAX as i128 {
                    Some(val as i16)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl SurrealType for i32 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => {
                let val = i128::from(i);
                if val >= i32::MIN as i128 && val <= i32::MAX as i128 {
                    Some(val as i32)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl SurrealType for i64 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => {
                let val = i128::from(i);
                if val >= i64::MIN as i128 && val <= i64::MAX as i128 {
                    Some(val as i64)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl SurrealType for isize {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => {
                let val = i128::from(i);
                if val >= isize::MIN as i128 && val <= isize::MAX as i128 {
                    Some(val as isize)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl SurrealType for u8 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => {
                let val = i128::from(i);
                if val >= 0 && val <= u8::MAX as i128 {
                    Some(val as u8)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl SurrealType for u16 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => {
                let val = i128::from(i);
                if val >= 0 && val <= u16::MAX as i128 {
                    Some(val as u16)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl SurrealType for u32 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => {
                let val = i128::from(i);
                if val >= 0 && val <= u32::MAX as i128 {
                    Some(val as u32)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl SurrealType for u64 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => {
                let val = i128::from(i);
                if val >= 0 && val <= u64::MAX as i128 {
                    Some(val as u64)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl SurrealType for usize {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => {
                let val = i128::from(i);
                if val >= 0 && val <= usize::MAX as i128 {
                    Some(val as usize)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

// Floating point types - all map to Float
impl SurrealType for f32 {
    type Target = SurrealTypeFloatMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Float(*self as f64)
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Float(f) => Some(f as f32),
            CborValue::Integer(i) => Some(i128::from(i) as f32),
            _ => None,
        }
    }
}

impl SurrealType for f64 {
    type Target = SurrealTypeFloatMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Float(*self)
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Float(f) => Some(f),
            CborValue::Integer(i) => Some(i128::from(i) as f64),
            _ => None,
        }
    }
}

// Boolean type
impl SurrealType for bool {
    type Target = SurrealTypeBoolMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Bool(*self)
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Bool(b) => Some(b),
            _ => None,
        }
    }
}

/// SurrealDB Record ID type
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RId {
    pub table: String,
    pub id: String,
}

impl RId {
    pub fn new(table: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            id: id.into(),
        }
    }

    pub fn from_string(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.splitn(2, ':').collect();
        if parts.len() == 2 {
            Some(RId::new(parts[0], parts[1]))
        } else {
            None
        }
    }
}

impl std::fmt::Display for RId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.table, self.id)
    }
}

impl SurrealType for RId {
    type Target = SurrealTypeStringMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Text(self.to_string())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Text(s) => RId::from_string(&s),
            _ => None,
        }
    }
}

// Duration implementations
impl SurrealType for std::time::Duration {
    type Target = SurrealTypeDurationMarker;

    fn to_cbor(&self) -> CborValue {
        let secs = self.as_secs();
        let nanos = self.subsec_nanos();
        CborValue::Tag(
            14,
            Box::new(CborValue::Array(vec![
                CborValue::Integer(secs.into()),
                CborValue::Integer(nanos.into()),
            ])),
        )
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Tag(14, boxed_value) => {
                if let CborValue::Array(arr) = boxed_value.as_ref()
                    && arr.len() == 2
                    && let (CborValue::Integer(secs), CborValue::Integer(nanos)) =
                        (&arr[0], &arr[1])
                {
                    let seconds = i128::from(*secs);
                    let nanos = i128::from(*nanos) as u32;
                    // std::time::Duration cannot represent negative durations
                    if seconds >= 0 {
                        return Some(std::time::Duration::new(seconds as u64, nanos));
                    }
                }
                None
            }
            _ => None,
        }
    }
}

impl SurrealType for chrono::Duration {
    type Target = SurrealTypeDurationMarker;

    fn to_cbor(&self) -> CborValue {
        let secs = self.num_seconds();
        let nanos = self.num_nanoseconds().unwrap_or(0) % 1_000_000_000;
        CborValue::Tag(
            14,
            Box::new(CborValue::Array(vec![
                CborValue::Integer(secs.into()),
                CborValue::Integer(nanos.into()),
            ])),
        )
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Tag(14, boxed_value) => {
                if let CborValue::Array(arr) = boxed_value.as_ref()
                    && arr.len() == 2
                    && let (CborValue::Integer(secs), CborValue::Integer(nanos)) =
                        (&arr[0], &arr[1])
                {
                    let seconds = i128::from(*secs);
                    let nanos = i128::from(*nanos) as i32;
                    return Some(
                        chrono::Duration::seconds(seconds as i64)
                            + chrono::Duration::nanoseconds(nanos as i64),
                    );
                }
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_types() {
        let s = "hello".to_string();
        let cbor = s.to_cbor();
        assert_eq!(cbor, CborValue::Text("hello".to_string()));

        let restored = String::from_cbor(cbor).unwrap();
        assert_eq!(restored, "hello");
    }

    #[test]
    fn test_integer_types() {
        let i32_val: i32 = 42;
        let cbor = i32_val.to_cbor();
        let restored = i32::from_cbor(cbor).unwrap();
        assert_eq!(restored, 42);

        let i64_val: i64 = 42;
        let cbor = i64_val.to_cbor();
        let restored = i64::from_cbor(cbor).unwrap();
        assert_eq!(restored, 42);
    }

    #[test]
    fn test_float_types() {
        let f64_val: f64 = 42.5;
        let cbor = f64_val.to_cbor();
        let restored = f64::from_cbor(cbor).unwrap();
        assert_eq!(restored, 42.5);
    }

    #[test]
    fn test_bool_type() {
        let b = true;
        let cbor = b.to_cbor();
        let restored = bool::from_cbor(cbor).unwrap();
        assert!(restored);

        let b2 = false;
        let cbor = b2.to_cbor();
        let restored = bool::from_cbor(cbor).unwrap();
        assert!(!restored);
    }

    #[test]
    fn test_any_type() {
        let any = Any;
        let cbor = any.to_cbor();
        let restored = Any::from_cbor(cbor).unwrap();
        // Any should round-trip successfully
        assert_eq!(format!("{:?}", any), format!("{:?}", restored));
    }

    #[test]
    fn test_rid_type_uses_string_marker() {
        let rid = RId::new("user", "123");
        let cbor = rid.to_cbor();

        // RId should serialize as CBOR text
        assert_eq!(cbor, CborValue::Text("user:123".to_string()));

        // Should round-trip correctly
        let restored = RId::from_cbor(cbor).unwrap();
        assert_eq!(rid, restored);
        assert_eq!(rid.table, "user");
        assert_eq!(rid.id, "123");
        assert_eq!(rid.to_string(), "user:123");

        // Should also parse from string format
        let from_string = RId::from_string("product:abc").unwrap();
        assert_eq!(from_string.table, "product");
        assert_eq!(from_string.id, "abc");
    }

    #[test]
    fn test_chrono_duration_precision() {
        // Test that nanosecond precision is preserved
        let dur = chrono::Duration::nanoseconds(1_234_567_890); // 1.23456789 seconds
        let cbor = dur.to_cbor();
        let restored = chrono::Duration::from_cbor(cbor).unwrap();

        // Should preserve the nanosecond precision
        assert_eq!(dur.num_nanoseconds(), restored.num_nanoseconds());
        assert_eq!(dur.num_seconds(), restored.num_seconds());
    }

    #[test]
    fn test_negative_timestamp_handling() {
        use chrono::{TimeZone, Timelike};

        // Test date before Unix epoch (1960-01-01)
        let past_date = chrono::Utc.with_ymd_and_hms(1960, 1, 1, 0, 0, 0).unwrap();
        let cbor = past_date.to_cbor();
        let restored = chrono::DateTime::<chrono::Utc>::from_cbor(cbor).unwrap();

        // Should preserve the date even though timestamp is negative
        assert_eq!(past_date.timestamp(), restored.timestamp());
        assert_eq!(past_date, restored);

        // Test date with nanoseconds before Unix epoch
        let past_date_nanos = chrono::Utc
            .with_ymd_and_hms(1969, 12, 31, 23, 59, 59)
            .unwrap()
            .with_nanosecond(500_000_000)
            .unwrap();
        let cbor_nanos = past_date_nanos.to_cbor();
        let restored_nanos = chrono::DateTime::<chrono::Utc>::from_cbor(cbor_nanos).unwrap();

        // Should preserve nanosecond precision for negative timestamps
        assert_eq!(past_date_nanos.timestamp(), restored_nanos.timestamp());
        assert_eq!(
            past_date_nanos.timestamp_nanos_opt(),
            restored_nanos.timestamp_nanos_opt()
        );
        assert_eq!(past_date_nanos, restored_nanos);

        // Test SystemTime with negative timestamp
        let system_time_past = std::time::UNIX_EPOCH - std::time::Duration::from_secs(315_360_000); // ~1960
        let cbor_st = system_time_past.to_cbor();
        let restored_st = std::time::SystemTime::from_cbor(cbor_st).unwrap();

        // Should preserve the time even for dates before epoch
        assert!(
            system_time_past
                .duration_since(std::time::UNIX_EPOCH)
                .is_err()
        ); // Negative duration
        assert_eq!(system_time_past, restored_st);
    }

    #[test]
    fn test_negative_duration_handling() {
        // Test that negative durations are rejected for std::time::Duration
        use ciborium::value::Value as CborValue;

        // Create CBOR for negative duration
        let negative_duration_cbor = CborValue::Tag(
            14,
            Box::new(CborValue::Array(vec![
                CborValue::Integer((-30i64).into()), // -30 seconds
                CborValue::Integer(0i64.into()),
            ])),
        );

        // Should return None for negative std::time::Duration
        let result = std::time::Duration::from_cbor(negative_duration_cbor);
        assert!(
            result.is_none(),
            "Negative duration should be rejected for std::time::Duration"
        );

        // But chrono::Duration should handle negative values
        let chrono_negative = chrono::Duration::seconds(-30);
        let cbor = chrono_negative.to_cbor();
        let restored = chrono::Duration::from_cbor(cbor).unwrap();
        assert_eq!(chrono_negative.num_seconds(), restored.num_seconds());
    }
}
