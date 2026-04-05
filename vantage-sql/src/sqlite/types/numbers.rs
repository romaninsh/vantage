//! Numeric type implementations for SQLite.
//!
//! - Integer types (i8..i64, u8..u32) → SqliteType::Integer
//! - Float types (f32, f64) → SqliteType::Real
//! - Option<T> delegates to T, with Null for None

use super::{SqliteType, SqliteTypeIntegerMarker, SqliteTypeRealMarker};
use serde_json::Value;

// -- Signed integers → Integer affinity ------------------------------------

impl SqliteType for i64 {
    type Target = SqliteTypeIntegerMarker;

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

impl SqliteType for i32 {
    type Target = SqliteTypeIntegerMarker;

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

impl SqliteType for i16 {
    type Target = SqliteTypeIntegerMarker;

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

impl SqliteType for i8 {
    type Target = SqliteTypeIntegerMarker;

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

// -- Unsigned integers → Integer affinity ----------------------------------

impl SqliteType for u32 {
    type Target = SqliteTypeIntegerMarker;

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

impl SqliteType for u16 {
    type Target = SqliteTypeIntegerMarker;

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

impl SqliteType for u8 {
    type Target = SqliteTypeIntegerMarker;

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

// -- Float types → Real affinity -------------------------------------------

impl SqliteType for f64 {
    type Target = SqliteTypeRealMarker;

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

impl SqliteType for f32 {
    type Target = SqliteTypeRealMarker;

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

// -- Option<T> → Null for None, delegates to T for Some --------------------

impl<T: SqliteType> SqliteType for Option<T> {
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
