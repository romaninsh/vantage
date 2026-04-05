//! Boolean → SQLite Bool variant.
//! Stored as INTEGER 0/1 on disk, but a distinct type at the vantage level.

use super::{SqliteType, SqliteTypeBoolMarker};
use serde_json::Value;

impl SqliteType for bool {
    type Target = SqliteTypeBoolMarker;

    fn to_json(&self) -> Value {
        // Store as integer 0/1 matching SQLite's convention
        Value::Number(if *self { 1.into() } else { 0.into() })
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().map(|i| i != 0),
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }
}
