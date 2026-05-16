//! Chrono integration — most GraphQL servers expose datetime custom
//! scalars as RFC 3339 strings.

use super::{GraphqlType, GraphqlTypeDateMarker, GraphqlTypeDateTimeMarker, GraphqlTypeTimeMarker};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime, Utc};
use serde_json::Value;

impl GraphqlType for DateTime<Utc> {
    type Target = GraphqlTypeDateTimeMarker;

    fn to_json(&self) -> Value {
        Value::String(self.to_rfc3339())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::String(s) => DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc)),
            _ => None,
        }
    }
}

impl GraphqlType for DateTime<FixedOffset> {
    type Target = GraphqlTypeDateTimeMarker;

    fn to_json(&self) -> Value {
        Value::String(self.to_rfc3339())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::String(s) => DateTime::parse_from_rfc3339(&s).ok(),
            _ => None,
        }
    }
}

impl GraphqlType for NaiveDate {
    type Target = GraphqlTypeDateMarker;

    fn to_json(&self) -> Value {
        Value::String(self.format("%Y-%m-%d").to_string())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::String(s) => NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok(),
            _ => None,
        }
    }
}

impl GraphqlType for NaiveTime {
    type Target = GraphqlTypeTimeMarker;

    fn to_json(&self) -> Value {
        Value::String(self.format("%H:%M:%S%.f").to_string())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::String(s) => NaiveTime::parse_from_str(&s, "%H:%M:%S")
                .or_else(|_| NaiveTime::parse_from_str(&s, "%H:%M:%S%.f"))
                .ok(),
            _ => None,
        }
    }
}
