//! String types -> MySQL TEXT

use super::{MysqlType, MysqlTypeTextMarker};
use serde_json::Value;

impl MysqlType for String {
    type Target = MysqlTypeTextMarker;

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

impl MysqlType for &'static str {
    type Target = MysqlTypeTextMarker;

    fn to_json(&self) -> Value {
        Value::String(self.to_string())
    }

    fn from_json(_value: Value) -> Option<Self> {
        None // cannot produce &'static str from arbitrary data
    }
}
