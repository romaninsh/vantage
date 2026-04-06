//! Boolean -> MySQL Bool variant.
//! MySQL has BOOLEAN type (alias for TINYINT(1)).

use super::{MysqlType, MysqlTypeBoolMarker};
use serde_json::Value;

impl MysqlType for bool {
    type Target = MysqlTypeBoolMarker;

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
