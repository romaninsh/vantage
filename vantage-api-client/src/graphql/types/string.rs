use super::{GraphqlType, GraphqlTypeStringMarker};
use serde_json::Value;

impl GraphqlType for String {
    type Target = GraphqlTypeStringMarker;

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

impl GraphqlType for &'static str {
    type Target = GraphqlTypeStringMarker;

    fn to_json(&self) -> Value {
        Value::String((*self).to_string())
    }

    fn from_json(_value: Value) -> Option<Self> {
        None
    }
}
