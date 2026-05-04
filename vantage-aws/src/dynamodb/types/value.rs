//! `AnyDynamoType` extras: untyped constructor, `From` impls, `Expressive`.

use super::{AnyDynamoType, AttributeValue, DynamoType, DynamoTypeLMarker, DynamoTypeNullMarker};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

impl AnyDynamoType {
    /// Build a value with no variant tag — used for AttributeValues
    /// arriving from DynamoDB, where we trust the wire shape and let
    /// `try_get` attempt the read without enforcing a tag.
    pub fn untyped(value: AttributeValue) -> Self {
        Self {
            value,
            type_variant: None,
        }
    }
}

/// `AnyDynamoType` is itself a `DynamoType` — passthrough.
impl DynamoType for AnyDynamoType {
    type Target = DynamoTypeNullMarker;

    fn to_attr(&self) -> AttributeValue {
        self.value().clone()
    }

    fn from_attr(value: AttributeValue) -> Option<Self> {
        AnyDynamoType::from_attr(&value)
    }
}

/// `Vec<AnyDynamoType>` — used by `column_table_values_expr` to project
/// a column's values across rows.
impl DynamoType for Vec<AnyDynamoType> {
    type Target = DynamoTypeLMarker;

    fn to_attr(&self) -> AttributeValue {
        AttributeValue::L(self.iter().map(|v| v.value().clone()).collect())
    }

    fn from_attr(value: AttributeValue) -> Option<Self> {
        match value {
            AttributeValue::L(arr) => Some(arr.into_iter().map(AnyDynamoType::untyped).collect()),
            _ => None,
        }
    }
}

macro_rules! impl_from_for_any_dynamo {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for AnyDynamoType {
                fn from(val: $ty) -> Self {
                    AnyDynamoType::new(val)
                }
            }
        )*
    };
}

impl_from_for_any_dynamo!(i32, i64, f64, bool, String, Vec<u8>);

impl From<&str> for AnyDynamoType {
    fn from(val: &str) -> Self {
        AnyDynamoType::new(val.to_string())
    }
}

impl From<crate::dynamodb::id::DynamoId> for AnyDynamoType {
    fn from(val: crate::dynamodb::id::DynamoId) -> Self {
        AnyDynamoType::new(val.into_string())
    }
}

// Expressive impls — pass scalars directly into dynamo expressions.
macro_rules! impl_expressive_for_dynamo_scalar {
    ($($ty:ty),*) => {
        $(
            impl Expressive<AnyDynamoType> for $ty {
                fn expr(&self) -> Expression<AnyDynamoType> {
                    Expression::new(
                        "{}",
                        vec![ExpressiveEnum::Scalar(AnyDynamoType::new_ref(self))],
                    )
                }
            }
        )*
    };
}

impl_expressive_for_dynamo_scalar!(i32, i64, f64, bool, String);

impl Expressive<AnyDynamoType> for &str {
    fn expr(&self) -> Expression<AnyDynamoType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnyDynamoType::from(*self))],
        )
    }
}

impl Expressive<AnyDynamoType> for AnyDynamoType {
    fn expr(&self) -> Expression<AnyDynamoType> {
        Expression::new("{}", vec![ExpressiveEnum::Scalar(self.clone())])
    }
}
