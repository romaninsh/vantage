//! Numeric type implementations for GraphQL.
//!
//! - i32 → GraphqlType::Int (spec scalar, 32-bit signed)
//! - i64 → GraphqlType::BigInt (custom scalar, 64-bit signed)
//! - f64 → GraphqlType::Float

use super::{GraphqlType, GraphqlTypeBigIntMarker, GraphqlTypeFloatMarker, GraphqlTypeIntMarker};
use serde_json::Value;

impl GraphqlType for i32 {
    type Target = GraphqlTypeIntMarker;

    fn to_json(&self) -> Value {
        Value::Number((*self).into())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().and_then(|i| i32::try_from(i).ok()),
            _ => None,
        }
    }
}

impl GraphqlType for i64 {
    type Target = GraphqlTypeBigIntMarker;

    fn to_json(&self) -> Value {
        Value::Number((*self).into())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64(),
            // Some servers serialise BigInt as a quoted string to dodge
            // JavaScript's 53-bit precision ceiling. Accept that form too.
            Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl GraphqlType for f64 {
    type Target = GraphqlTypeFloatMarker;

    fn to_json(&self) -> Value {
        serde_json::Number::from_f64(*self)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_f64(),
            _ => None,
        }
    }
}

impl<T: GraphqlType> GraphqlType for Option<T> {
    type Target = T::Target;

    fn to_json(&self) -> Value {
        match self {
            Some(v) => v.to_json(),
            None => Value::Null,
        }
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Null => Some(None),
            other => T::from_json(other).map(Some),
        }
    }
}
