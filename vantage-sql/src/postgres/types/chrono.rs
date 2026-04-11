//! Chrono type implementations for PostgreSQL.
//!
//! CBOR tags for type preservation:
//!   Tag(100) = Date (YYYY-MM-DD)
//!   Tag(101) = Time (HH:MM:SS)
//!   Tag(0)   = DateTime (ISO 8601 / RFC 3339)
//!
//! `from_cbor` accepts both tagged and plain Text values.

use super::{
    PostgresType, PostgresTypeDateMarker, PostgresTypeDateTimeMarker, PostgresTypeTimeMarker,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
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
            Box::new(Value::Text(self.format("%H:%M:%S").to_string())),
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
        Value::Tag(
            0,
            Box::new(Value::Text(self.format("%Y-%m-%dT%H:%M:%S").to_string())),
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
        Value::Tag(
            0,
            Box::new(Value::Text(
                self.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
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
