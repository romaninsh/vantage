//! Chrono type implementations for SQLite.
//!
//! SQLite stores all dates as TEXT. CBOR tags for type preservation:
//!   Tag(100) = Date (YYYY-MM-DD)
//!   Tag(101) = Time (HH:MM:SS)
//!   Tag(0)   = DateTime
//!
//! **Format**: Uses ISO 8601 with T separator (`"2025-01-10T12:00:00Z"`)
//! since SQLite has no native date type — the format is ours to choose.
//!
//! `from_cbor` accepts both tagged and plain Text values, so dates read
//! from SQLite (which come back as untagged strings) still work.

use super::{SqliteType, SqliteTypeTextMarker};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use ciborium::Value;

impl SqliteType for NaiveDate {
    type Target = SqliteTypeTextMarker;

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
                    // Try date-only first, then strip time from datetime
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

impl SqliteType for NaiveTime {
    type Target = SqliteTypeTextMarker;

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

impl SqliteType for NaiveDateTime {
    type Target = SqliteTypeTextMarker;

    fn to_cbor(&self) -> Value {
        Value::Tag(
            0,
            Box::new(Value::Text(self.format("%Y-%m-%dT%H:%M:%S%.f").to_string())),
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

impl SqliteType for DateTime<Utc> {
    type Target = SqliteTypeTextMarker;

    fn to_cbor(&self) -> Value {
        Value::Tag(
            0,
            Box::new(Value::Text(
                self.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
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

impl SqliteType for DateTime<FixedOffset> {
    type Target = SqliteTypeTextMarker;

    fn to_cbor(&self) -> Value {
        Value::Tag(
            0,
            Box::new(Value::Text(
                self.format("%Y-%m-%dT%H:%M:%S%.f%:z").to_string(),
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

fn parse_naive_datetime(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f"))
        .ok()
}

fn parse_datetime_utc(s: &str) -> Option<DateTime<Utc>> {
    s.parse::<DateTime<Utc>>()
        .or_else(|_| {
            // Try parsing as naive datetime and assume UTC
            parse_naive_datetime(s).map(|ndt| ndt.and_utc()).ok_or(())
        })
        .ok()
}

fn parse_datetime_fixed(s: &str) -> Option<DateTime<FixedOffset>> {
    s.parse::<DateTime<FixedOffset>>()
        .ok()
        .or_else(|| {
            DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%:z")
                .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%:z"))
                .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%#z"))
                .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%#z"))
                .ok()
        })
        // Naive: assume UTC
        .or_else(|| {
            parse_naive_datetime(s).map(|ndt| ndt.and_utc().fixed_offset())
        })
}
