//! String → DynamoDB `S` AttributeValue.

use super::{AttributeValue, DynamoType, DynamoTypeSMarker};

impl DynamoType for String {
    type Target = DynamoTypeSMarker;

    fn to_attr(&self) -> AttributeValue {
        AttributeValue::S(self.clone())
    }

    fn from_attr(value: AttributeValue) -> Option<Self> {
        match value {
            AttributeValue::S(s) => Some(s),
            _ => None,
        }
    }
}

impl DynamoType for &'static str {
    type Target = DynamoTypeSMarker;

    fn to_attr(&self) -> AttributeValue {
        AttributeValue::S(self.to_string())
    }

    fn from_attr(_value: AttributeValue) -> Option<Self> {
        None
    }
}
