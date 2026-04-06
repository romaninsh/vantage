//! Numeric type implementations for PostgreSQL.
//!
//! - i16 -> Int2 (SMALLINT)
//! - i32 -> Int4 (INTEGER)
//! - i64 -> Int8 (BIGINT)
//! - f32 -> Float4 (REAL)
//! - f64 -> Float8 (DOUBLE PRECISION)
//! - i8, u8, u16, u32 -> mapped to appropriate integer variants
//! - Option<T> delegates to T, with Null for None

use super::{
    PostgresType, PostgresTypeFloat4Marker, PostgresTypeFloat8Marker, PostgresTypeInt2Marker,
    PostgresTypeInt4Marker, PostgresTypeInt8Marker,
};
use serde_json::Value;

// -- i16 -> Int2 (SMALLINT) ---------------------------------------------------

impl PostgresType for i16 {
    type Target = PostgresTypeInt2Marker;

    fn to_json(&self) -> Value {
        Value::Number((*self as i64).into())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().and_then(|i| i16::try_from(i).ok()),
            _ => None,
        }
    }
}

// -- i32 -> Int4 (INTEGER) ----------------------------------------------------

impl PostgresType for i32 {
    type Target = PostgresTypeInt4Marker;

    fn to_json(&self) -> Value {
        Value::Number((*self as i64).into())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().and_then(|i| i32::try_from(i).ok()),
            _ => None,
        }
    }
}

// -- i64 -> Int8 (BIGINT) ----------------------------------------------------

impl PostgresType for i64 {
    type Target = PostgresTypeInt8Marker;

    fn to_json(&self) -> Value {
        Value::Number((*self).into())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64(),
            _ => None,
        }
    }
}

// -- Smaller unsigned integers mapped to appropriate Postgres types -----------

impl PostgresType for i8 {
    type Target = PostgresTypeInt2Marker;

    fn to_json(&self) -> Value {
        Value::Number((*self as i64).into())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().and_then(|i| i8::try_from(i).ok()),
            _ => None,
        }
    }
}

impl PostgresType for u8 {
    type Target = PostgresTypeInt2Marker;

    fn to_json(&self) -> Value {
        Value::Number((*self as i64).into())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().and_then(|i| u8::try_from(i).ok()),
            _ => None,
        }
    }
}

impl PostgresType for u16 {
    type Target = PostgresTypeInt4Marker;

    fn to_json(&self) -> Value {
        Value::Number((*self as i64).into())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().and_then(|i| u16::try_from(i).ok()),
            _ => None,
        }
    }
}

impl PostgresType for u32 {
    type Target = PostgresTypeInt8Marker;

    fn to_json(&self) -> Value {
        Value::Number((*self as i64).into())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().and_then(|i| u32::try_from(i).ok()),
            _ => None,
        }
    }
}

// -- Float types --------------------------------------------------------------

impl PostgresType for f32 {
    type Target = PostgresTypeFloat4Marker;

    fn to_json(&self) -> Value {
        serde_json::Number::from_f64(*self as f64)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_f64().map(|f| f as f32),
            _ => None,
        }
    }
}

impl PostgresType for f64 {
    type Target = PostgresTypeFloat8Marker;

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

// -- Option<T> -> Null for None, delegates to T for Some ----------------------

impl<T: PostgresType> PostgresType for Option<T> {
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
