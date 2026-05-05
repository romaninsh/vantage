//! Bool → DynamoDB `BOOL` AttributeValue.

use super::{AttributeValue, DynamoType, DynamoTypeBoolMarker};

impl DynamoType for bool {
    type Target = DynamoTypeBoolMarker;

    fn to_attr(&self) -> AttributeValue {
        AttributeValue::Bool(*self)
    }

    fn from_attr(value: AttributeValue) -> Option<Self> {
        match value {
            AttributeValue::Bool(b) => Some(b),
            _ => None,
        }
    }
}
