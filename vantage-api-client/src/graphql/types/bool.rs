use super::{GraphqlType, GraphqlTypeBoolMarker};
use serde_json::Value;

impl GraphqlType for bool {
    type Target = GraphqlTypeBoolMarker;

    fn to_json(&self) -> Value {
        Value::Bool(*self)
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }
}
