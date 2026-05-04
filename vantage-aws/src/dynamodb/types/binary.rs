//! Bytes → DynamoDB `B` AttributeValue.

use super::{AttributeValue, DynamoType, DynamoTypeBMarker};

impl DynamoType for Vec<u8> {
    type Target = DynamoTypeBMarker;

    fn to_attr(&self) -> AttributeValue {
        AttributeValue::B(self.clone())
    }

    fn from_attr(value: AttributeValue) -> Option<Self> {
        match value {
            AttributeValue::B(b) => Some(b),
            _ => None,
        }
    }
}
