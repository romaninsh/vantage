//! Numeric types map to DynamoDB's `N` AttributeValue.
//!
//! DynamoDB sends/receives numbers as strings to keep precision intact;
//! the parse happens here, not on the wire.

use super::{AttributeValue, DynamoType, DynamoTypeNMarker};

impl DynamoType for i32 {
    type Target = DynamoTypeNMarker;

    fn to_attr(&self) -> AttributeValue {
        AttributeValue::N(self.to_string())
    }

    fn from_attr(value: AttributeValue) -> Option<Self> {
        match value {
            AttributeValue::N(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl DynamoType for i64 {
    type Target = DynamoTypeNMarker;

    fn to_attr(&self) -> AttributeValue {
        AttributeValue::N(self.to_string())
    }

    fn from_attr(value: AttributeValue) -> Option<Self> {
        match value {
            AttributeValue::N(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl DynamoType for f64 {
    type Target = DynamoTypeNMarker;

    fn to_attr(&self) -> AttributeValue {
        AttributeValue::N(self.to_string())
    }

    fn from_attr(value: AttributeValue) -> Option<Self> {
        match value {
            AttributeValue::N(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl<T: DynamoType> DynamoType for Option<T> {
    type Target = T::Target;

    fn to_attr(&self) -> AttributeValue {
        match self {
            Some(v) => v.to_attr(),
            None => AttributeValue::Null,
        }
    }

    fn from_attr(value: AttributeValue) -> Option<Self> {
        match value {
            AttributeValue::Null => Some(None),
            other => T::from_attr(other).map(Some),
        }
    }
}
