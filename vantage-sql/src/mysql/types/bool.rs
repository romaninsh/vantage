//! Boolean -> MySQL Bool variant.
//! MySQL has BOOLEAN type (alias for TINYINT(1)).

use super::{MysqlType, MysqlTypeBoolMarker};
use ciborium::Value;

impl MysqlType for bool {
    type Target = MysqlTypeBoolMarker;

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
