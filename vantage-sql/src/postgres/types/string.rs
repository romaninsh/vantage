//! String types -> PostgreSQL TEXT

use super::{PostgresType, PostgresTypeTextMarker};
use ciborium::Value;

impl PostgresType for String {
    type Target = PostgresTypeTextMarker;

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

impl PostgresType for &'static str {
    type Target = PostgresTypeTextMarker;

    fn to_cbor(&self) -> Value {
        Value::Text(self.to_string())
    }

    fn from_cbor(_value: Value) -> Option<Self> {
        None // cannot produce &'static str from arbitrary data
    }
}
