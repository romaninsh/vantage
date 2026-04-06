//! String types -> PostgreSQL TEXT

use super::{PostgresType, PostgresTypeTextMarker};
use serde_json::Value;

impl PostgresType for String {
    type Target = PostgresTypeTextMarker;

    fn to_json(&self) -> Value {
        Value::String(self.clone())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

impl PostgresType for &'static str {
    type Target = PostgresTypeTextMarker;

    fn to_json(&self) -> Value {
        Value::String(self.to_string())
    }

    fn from_json(_value: Value) -> Option<Self> {
        None // cannot produce &'static str from arbitrary data
    }
}
