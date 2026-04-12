//! Numeric type implementations for SQLite.
//!
//! Uses ciborium::Value (CBOR) as the underlying storage:
//! - Integer types (i8..i64, u8..u32) → CborValue::Integer
//! - Float types (f32, f64) → CborValue::Float
//! - Option<T> delegates to T, with Null for None
//!
//! `from_cbor` accepts Text as a fallback — allows extraction from TEXT
//! columns where the database returns strings instead of native numeric types.
//! No cross-conversion between Integer and Float — use the matching type.

use super::{SqliteType, SqliteTypeIntegerMarker, SqliteTypeRealMarker};
use ciborium::Value;

// -- Signed integers → Integer affinity ------------------------------------

impl SqliteType for i64 {
    type Target = SqliteTypeIntegerMarker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => i64::try_from(i).ok(),
            Value::Text(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl SqliteType for i32 {
    type Target = SqliteTypeIntegerMarker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => i32::try_from(i).ok(),
            Value::Text(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl SqliteType for i16 {
    type Target = SqliteTypeIntegerMarker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => i16::try_from(i).ok(),
            Value::Text(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl SqliteType for i8 {
    type Target = SqliteTypeIntegerMarker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => i8::try_from(i).ok(),
            Value::Text(s) => s.parse().ok(),
            _ => None,
        }
    }
}

// -- Unsigned integers → Integer affinity ----------------------------------

impl SqliteType for u32 {
    type Target = SqliteTypeIntegerMarker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => u32::try_from(i).ok(),
            Value::Text(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl SqliteType for u16 {
    type Target = SqliteTypeIntegerMarker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => u16::try_from(i).ok(),
            Value::Text(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl SqliteType for u8 {
    type Target = SqliteTypeIntegerMarker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => u8::try_from(i).ok(),
            Value::Text(s) => s.parse().ok(),
            _ => None,
        }
    }
}

// -- Float types → Real affinity -------------------------------------------

impl SqliteType for f64 {
    type Target = SqliteTypeRealMarker;

    fn to_cbor(&self) -> Value {
        Value::Float(*self)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Float(f) => Some(f),
            Value::Text(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl SqliteType for f32 {
    type Target = SqliteTypeRealMarker;

    fn to_cbor(&self) -> Value {
        Value::Float((*self) as f64)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Float(f) => Some(f as f32),
            Value::Text(s) => s.parse().ok(),
            _ => None,
        }
    }
}

// -- Option<T> → Null for None, delegates to T for Some --------------------

impl<T: SqliteType> SqliteType for Option<T> {
    type Target = T::Target;

    fn to_cbor(&self) -> Value {
        match self {
            Some(v) => v.to_cbor(),
            None => Value::Null,
        }
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Null => Some(None),
            other => T::from_cbor(other).map(Some),
        }
    }
}
