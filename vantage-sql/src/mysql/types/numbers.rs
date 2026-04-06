//! Numeric type implementations for MySQL.
//!
//! - i16 -> Int2 (SMALLINT)
//! - i32 -> Int4 (INT)
//! - i64 -> Int8 (BIGINT)
//! - f32 -> Float4 (FLOAT)
//! - f64 -> Float8 (DOUBLE)
//! - i8, u8, u16, u32 -> mapped to appropriate integer variants
//! - Option<T> delegates to T, with Null for None

use super::{
    MysqlType, MysqlTypeFloat4Marker, MysqlTypeFloat8Marker, MysqlTypeInt2Marker,
    MysqlTypeInt4Marker, MysqlTypeInt8Marker,
};
use serde_json::Value;

// -- i16 -> Int2 (SMALLINT) ---------------------------------------------------

impl MysqlType for i16 {
    type Target = MysqlTypeInt2Marker;

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

// -- i32 -> Int4 (INT) --------------------------------------------------------

impl MysqlType for i32 {
    type Target = MysqlTypeInt4Marker;

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

impl MysqlType for i64 {
    type Target = MysqlTypeInt8Marker;

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

// -- Smaller unsigned integers mapped to appropriate MySQL types ---------------

impl MysqlType for i8 {
    type Target = MysqlTypeInt2Marker;

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

impl MysqlType for u8 {
    type Target = MysqlTypeInt2Marker;

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

impl MysqlType for u16 {
    type Target = MysqlTypeInt4Marker;

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

impl MysqlType for u32 {
    type Target = MysqlTypeInt8Marker;

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

impl MysqlType for f32 {
    type Target = MysqlTypeFloat4Marker;

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

impl MysqlType for f64 {
    type Target = MysqlTypeFloat8Marker;

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

impl<T: MysqlType> MysqlType for Option<T> {
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
