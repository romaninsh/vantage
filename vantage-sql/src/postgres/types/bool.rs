//! Boolean -> PostgreSQL Bool variant.
//! PostgreSQL has native BOOLEAN type (unlike SQLite's 0/1).

use super::{PostgresType, PostgresTypeBoolMarker};
use serde_json::Value;

impl PostgresType for bool {
    type Target = PostgresTypeBoolMarker;

    fn to_json(&self) -> Value {
        Value::Bool(*self)
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Bool(b) => Some(b),
            Value::Number(n) => n.as_i64().map(|i| i != 0),
            _ => None,
        }
    }
}
