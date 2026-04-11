//! String types -> MySQL TEXT

use super::{MysqlType, MysqlTypeTextMarker};
use ciborium::Value;

impl MysqlType for String {
    type Target = MysqlTypeTextMarker;

    fn to_cbor(&self) -> Value {
        Value::Text(self.clone())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

impl MysqlType for &'static str {
    type Target = MysqlTypeTextMarker;

    fn to_cbor(&self) -> Value {
        Value::Text(self.to_string())
    }

    fn from_cbor(_value: Value) -> Option<Self> {
        None // cannot produce &'static str from arbitrary data
    }
}
