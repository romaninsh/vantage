//! Boolean → SQLite Bool variant.
//! Stored as INTEGER 0/1 on disk, but a distinct type at the vantage level.

use super::{SqliteType, SqliteTypeBoolMarker};
use ciborium::Value;

impl SqliteType for bool {
    type Target = SqliteTypeBoolMarker;

    fn to_cbor(&self) -> Value {
        // Store as integer 0/1 matching SQLite's convention
        Value::Integer(if *self { 1.into() } else { 0.into() })
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => i64::try_from(i).ok().map(|n| n != 0),
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }
}
