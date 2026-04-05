//! String types → SQLite TEXT affinity

use super::{SqliteType, SqliteTypeTextMarker};
use serde_json::Value;

impl SqliteType for String {
    type Target = SqliteTypeTextMarker;

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

impl SqliteType for &'static str {
    type Target = SqliteTypeTextMarker;

    fn to_json(&self) -> Value {
        Value::String(self.to_string())
    }

    fn from_json(_value: Value) -> Option<Self> {
        None // cannot produce &'static str from arbitrary data
    }
}
