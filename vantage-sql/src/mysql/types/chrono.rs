//! Chrono type implementations for MySQL.
//!
//! CBOR tags for type preservation:
//!   Tag(100) = Date (YYYY-MM-DD)
//!   Tag(101) = Time (HH:MM:SS)
//!   Tag(0)   = DateTime
//!
//! **Format**: MySQL uses `"2025-01-10 12:00:00"` (space separator, no T,
//! no timezone). DateTime<Utc> drops the timezone since MySQL DATETIME
//! doesn't store it (TIMESTAMP handles UTC conversion internally).
//!
//! `from_cbor` accepts both tagged and plain Text values, so values from
//! VARCHAR columns (untyped) can still be extracted as chrono types.

use super::{MysqlType, MysqlTypeDateMarker, MysqlTypeDateTimeMarker, MysqlTypeTimeMarker};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use ciborium::Value;

impl MysqlType for NaiveDate {
    type Target = MysqlTypeDateMarker;

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

impl MysqlType for NaiveTime {
    type Target = MysqlTypeTimeMarker;

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

impl MysqlType for NaiveDateTime {
    type Target = MysqlTypeDateTimeMarker;

    fn to_cbor(&self) -> Value {
        // MySQL-native format: space separator, no T
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

impl MysqlType for DateTime<Utc> {
    type Target = MysqlTypeDateTimeMarker;

    fn to_cbor(&self) -> Value {
        // MySQL-native format with fractional seconds
        Value::Tag(
            0,
            Box::new(Value::Text(self.format("%Y-%m-%d %H:%M:%S%.f").to_string())),
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

impl MysqlType for DateTime<FixedOffset> {
    type Target = MysqlTypeDateTimeMarker;

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

fn parse_naive_datetime(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f"))
        .ok()
}

fn parse_datetime_utc(s: &str) -> Option<DateTime<Utc>> {
    s.parse::<DateTime<Utc>>()
        .or_else(|_| parse_naive_datetime(s).map(|ndt| ndt.and_utc()).ok_or(()))
        .ok()
}

fn parse_datetime_fixed(s: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%:z")
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%:z"))
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%#z"))
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%#z"))
        .ok()
        .or_else(|| s.parse::<DateTime<FixedOffset>>().ok())
        // Naive: assume UTC
        .or_else(|| {
            parse_naive_datetime(s)
                .map(|ndt| ndt.and_utc().fixed_offset())
        })
}
