//! Boolean -> PostgreSQL Bool variant.
//! PostgreSQL has native BOOLEAN type (unlike SQLite's 0/1).

use super::{PostgresType, PostgresTypeBoolMarker};
use ciborium::Value;

impl PostgresType for bool {
    type Target = PostgresTypeBoolMarker;

    fn to_cbor(&self) -> Value {
        Value::Bool(*self)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Bool(b) => Some(b),
            Value::Integer(i) => i64::try_from(i).ok().map(|n| n != 0),
            _ => None,
        }
    }
}
