use super::{GraphqlType, GraphqlTypeUuidMarker};
use serde_json::Value;
use uuid::Uuid;

impl GraphqlType for Uuid {
    type Target = GraphqlTypeUuidMarker;

    fn to_json(&self) -> Value {
        Value::String(self.to_string())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::String(s) => Uuid::parse_str(&s).ok(),
            _ => None,
        }
    }
}
