//! Chrono type implementations for PostgreSQL.
//!
//! CBOR tags for type preservation:
//!   Tag(100) = Date (YYYY-MM-DD)
//!   Tag(101) = Time (HH:MM:SS)
//!   Tag(0)   = DateTime
//!
//! **Format**: Postgres uses `"2025-01-10 12:00:00+00"` (space separator,
//! abbreviated tz offset) — NOT RFC 3339. We store and parse in this format
//! so that what we write is what we read back, even through VARCHAR columns.
//!
//! `from_cbor` accepts both tagged and plain Text values, so values from
//! VARCHAR columns (untyped) can still be extracted as chrono types.

use super::{
    PostgresType, PostgresTypeDateMarker, PostgresTypeDateTimeMarker, PostgresTypeTimeMarker,
};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use ciborium::Value;

impl PostgresType for NaiveDate {
    type Target = PostgresTypeDateMarker;

    fn to_cbor(&self) -> Value {
        Value::Tag(
            100,
            Box::new(Value::Text(self.format("%Y-%m-%d").to_string())),
        )
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Tag(100, inner) | Value::Tag(0, inner) => {
                if let Value::Text(s) = *inner {
                    NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()
                } else {
                    None
                }
            }
            Value::Text(s) => NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok(),
            _ => None,
        }
    }
}

impl PostgresType for NaiveTime {
    type Target = PostgresTypeTimeMarker;

    fn to_cbor(&self) -> Value {
        Value::Tag(
            101,
            Box::new(Value::Text(self.format("%H:%M:%S%.f").to_string())),
        )
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Tag(101, inner) => {
                if let Value::Text(s) = *inner {
                    parse_time(&s)
                } else {
                    None
                }
            }
            Value::Text(s) => parse_time(&s),
            _ => None,
        }
    }
}

impl PostgresType for NaiveDateTime {
    type Target = PostgresTypeDateTimeMarker;

    fn to_cbor(&self) -> Value {
        // Postgres-native format: space separator, no T
        Value::Tag(
            0,
            Box::new(Value::Text(self.format("%Y-%m-%d %H:%M:%S%.f").to_string())),
        )
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Tag(0, inner) => {
                if let Value::Text(s) = *inner {
                    parse_naive_datetime(&s)
                } else {
                    None
                }
            }
            Value::Text(s) => parse_naive_datetime(&s),
            _ => None,
        }
    }
}

impl PostgresType for DateTime<Utc> {
    type Target = PostgresTypeDateTimeMarker;

    fn to_cbor(&self) -> Value {
        // Postgres-native format: "2025-01-10 12:00:00+00"
        // Matches what Postgres returns when storing DateTime into VARCHAR.
        Value::Tag(
            0,
            Box::new(Value::Text(
                self.format("%Y-%m-%d %H:%M:%S%.f+00").to_string(),
            )),
        )
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Tag(0, inner) => {
                if let Value::Text(s) = *inner {
                    parse_datetime_utc(&s)
                } else {
                    None
                }
            }
            Value::Text(s) => parse_datetime_utc(&s),
            _ => None,
        }
    }
}

impl PostgresType for DateTime<FixedOffset> {
    type Target = PostgresTypeDateTimeMarker;

    fn to_cbor(&self) -> Value {
        Value::Tag(
            0,
            Box::new(Value::Text(
                self.format("%Y-%m-%d %H:%M:%S%.f%:z").to_string(),
            )),
        )
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Tag(0, inner) => {
                if let Value::Text(s) = *inner {
                    parse_datetime_fixed(&s)
                } else {
                    None
                }
            }
            Value::Text(s) => parse_datetime_fixed(&s),
            _ => None,
        }
    }
}

fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M:%S%.f"))
        .ok()
}

/// Parse NaiveDateTime from Postgres-native or ISO formats.
/// Also handles TIMESTAMPTZ strings by stripping the timezone.
fn parse_naive_datetime(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f"))
        .ok()
        // Fallback: TIMESTAMPTZ format like "2025-01-10 12:00:00+00" — strip tz
        .or_else(|| parse_offset_datetime(s).map(|dt| dt.naive_utc()))
}

/// Parse `DateTime<Utc>` from Postgres-native format first (`+00` without colon),
/// then RFC 3339, then naive (assumed UTC).
fn parse_datetime_utc(s: &str) -> Option<DateTime<Utc>> {
    parse_offset_datetime(s).or_else(|| parse_naive_datetime(s).map(|ndt| ndt.and_utc()))
}

/// Text that carries an offset: Postgres-native (`+00` without colon,
/// which `%#z` accepts alongside `+00:00` and `+0000`), then RFC 3339.
fn parse_offset_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%#z")
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%#z"))
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| s.parse::<DateTime<Utc>>().ok())
}

fn parse_datetime_fixed(s: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%:z")
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%:z"))
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%#z"))
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%#z"))
        .ok()
        .or_else(|| s.parse::<DateTime<FixedOffset>>().ok())
        // Naive: assume UTC
        .or_else(|| parse_naive_datetime(s).map(|ndt| ndt.and_utc().fixed_offset()))
}
