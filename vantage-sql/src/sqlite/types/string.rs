//! String types → SQLite TEXT affinity

use super::{SqliteType, SqliteTypeTextMarker};
use ciborium::Value;

impl SqliteType for String {
    type Target = SqliteTypeTextMarker;

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

impl SqliteType for &'static str {
    type Target = SqliteTypeTextMarker;

    fn to_cbor(&self) -> Value {
        Value::Text(self.to_string())
    }

    fn from_cbor(_value: Value) -> Option<Self> {
        None // cannot produce &'static str from arbitrary data
    }
}
