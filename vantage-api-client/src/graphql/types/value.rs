//! AnyGraphqlType extras: untyped constructor, From impls, Expressive impls.

use super::{
    AnyGraphqlType, GraphqlType, GraphqlTypeArrayMarker, GraphqlTypeJsonMarker,
    GraphqlTypeNullMarker, GraphqlTypeObjectMarker,
};
use serde_json::Value;
use vantage_core::VantageError;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

impl AnyGraphqlType {
    /// Create an `AnyGraphqlType` with no type marker. Values coming back
    /// from a GraphQL response take this path — the wire format can't tell
    /// us whether a string is meant to be a `DateTime` or just a `String`,
    /// so `try_get::<T>()` is permissive on untyped values and the parser
    /// for `T` decides.
    pub fn untyped(value: Value) -> Self {
        Self {
            value,
            type_variant: None,
        }
    }
}

/// AnyGraphqlType is itself a GraphqlType — passthrough for type-erased values.
impl GraphqlType for AnyGraphqlType {
    type Target = GraphqlTypeNullMarker;

    fn to_json(&self) -> Value {
        self.value().clone()
    }

    fn from_json(value: Value) -> Option<Self> {
        AnyGraphqlType::from_json(&value)
    }
}

/// `serde_json::Value` passes through as the `Json` variant.
impl GraphqlType for Value {
    type Target = GraphqlTypeJsonMarker;

    fn to_json(&self) -> Value {
        self.clone()
    }

    fn from_json(value: Value) -> Option<Self> {
        Some(value)
    }
}

/// `Vec<AnyGraphqlType>` rendered as a JSON array.
impl GraphqlType for Vec<AnyGraphqlType> {
    type Target = GraphqlTypeArrayMarker;

    fn to_json(&self) -> Value {
        Value::Array(self.iter().map(|v| v.value().clone()).collect())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Array(arr) => Some(arr.into_iter().map(AnyGraphqlType::untyped).collect()),
            _ => None,
        }
    }
}

/// `indexmap::IndexMap` rendered as a JSON object — preserves field order.
impl GraphqlType for indexmap::IndexMap<String, AnyGraphqlType> {
    type Target = GraphqlTypeObjectMarker;

    fn to_json(&self) -> Value {
        let mut map = serde_json::Map::with_capacity(self.len());
        for (k, v) in self {
            map.insert(k.clone(), v.value().clone());
        }
        Value::Object(map)
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Object(obj) => Some(
                obj.into_iter()
                    .map(|(k, v)| (k, AnyGraphqlType::untyped(v)))
                    .collect(),
            ),
            _ => None,
        }
    }
}

impl TryFrom<AnyGraphqlType> for Vec<AnyGraphqlType> {
    type Error = VantageError;
    fn try_from(val: AnyGraphqlType) -> Result<Self, Self::Error> {
        val.try_get::<Vec<AnyGraphqlType>>().ok_or_else(|| {
            vantage_core::error!(
                "Cannot convert AnyGraphqlType to Vec<AnyGraphqlType>",
                value = format!("{}", val.value())
            )
        })
    }
}

// From impls for common Rust scalars.
macro_rules! impl_from_for_any_graphql {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for AnyGraphqlType {
                fn from(val: $ty) -> Self {
                    AnyGraphqlType::new(val)
                }
            }
        )*
    };
}

impl_from_for_any_graphql!(i32, i64, f64, bool, String);

impl From<&str> for AnyGraphqlType {
    fn from(val: &str) -> Self {
        AnyGraphqlType::new(val.to_string())
    }
}

// TryFrom<AnyGraphqlType> for common scalar types.
macro_rules! impl_try_from_graphql {
    ($($ty:ty),*) => {
        $(
            impl TryFrom<AnyGraphqlType> for $ty {
                type Error = VantageError;
                fn try_from(val: AnyGraphqlType) -> Result<Self, Self::Error> {
                    val.try_get::<$ty>().ok_or_else(|| {
                        vantage_core::error!(
                            "Cannot convert AnyGraphqlType to target type",
                            target = std::any::type_name::<$ty>(),
                            value = format!("{:?}", val.value())
                        )
                    })
                }
            }
        )*
    };
}

impl_try_from_graphql!(i32, i64, f64, bool, String);

// Expressive impls — let scalars flow directly into GraphQL expressions.
macro_rules! impl_expressive_for_graphql_scalar {
    ($($ty:ty),*) => {
        $(
            impl Expressive<AnyGraphqlType> for $ty {
                fn expr(&self) -> Expression<AnyGraphqlType> {
                    Expression::new(
                        "{}",
                        vec![ExpressiveEnum::Scalar(AnyGraphqlType::new_ref(self))],
                    )
                }
            }
        )*
    };
}

impl_expressive_for_graphql_scalar!(i32, i64, f64, bool, String);

impl Expressive<AnyGraphqlType> for &str {
    fn expr(&self) -> Expression<AnyGraphqlType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnyGraphqlType::from(*self))],
        )
    }
}

impl Expressive<AnyGraphqlType> for AnyGraphqlType {
    fn expr(&self) -> Expression<AnyGraphqlType> {
        Expression::new("{}", vec![ExpressiveEnum::Scalar(self.clone())])
    }
}

// `From<AnyGraphqlType> for serde_json::Value` and the reverse direction
// are generated by the `vantage_type_system!` macro. Only the CBOR
// bridges are needed manually — Vista speaks CBOR even though the wire
// is JSON, so we round-trip through serde at the boundary.
impl From<AnyGraphqlType> for ciborium::Value {
    fn from(val: AnyGraphqlType) -> Self {
        ciborium::Value::serialized(&val.into_value()).unwrap_or(ciborium::Value::Null)
    }
}

impl From<ciborium::Value> for AnyGraphqlType {
    fn from(val: ciborium::Value) -> Self {
        let json: Value = serde_json::to_value(&val).unwrap_or(Value::Null);
        AnyGraphqlType::from(json)
    }
}

impl std::fmt::Display for AnyGraphqlType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}
