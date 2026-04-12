//! Numeric type implementations for MySQL.
//!
//! Uses ciborium::Value (CBOR) as the underlying storage:
//! - Integers → CborValue::Integer
//! - Floats → CborValue::Float
//! - Option<T> delegates to T, with Null for None
//!
//! `from_cbor` accepts Text as a fallback — allows extraction from VARCHAR
//! columns where the database returns strings instead of native numeric types.
//! No cross-conversion between Integer and Float — use the matching type.

use super::{
    MysqlType, MysqlTypeFloat4Marker, MysqlTypeFloat8Marker, MysqlTypeInt2Marker,
    MysqlTypeInt4Marker, MysqlTypeInt8Marker,
};
use ciborium::Value;

// -- i16 -> Int2 (SMALLINT) ---------------------------------------------------

impl MysqlType for i16 {
    type Target = MysqlTypeInt2Marker;

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

// -- i32 -> Int4 (INT) --------------------------------------------------------

impl MysqlType for i32 {
    type Target = MysqlTypeInt4Marker;

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

// -- i64 -> Int8 (BIGINT) ----------------------------------------------------

impl MysqlType for i64 {
    type Target = MysqlTypeInt8Marker;

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

// -- Smaller integer types mapped to appropriate MySQL variants ---------------

impl MysqlType for i8 {
    type Target = MysqlTypeInt2Marker;

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

impl MysqlType for u8 {
    type Target = MysqlTypeInt2Marker;

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

impl MysqlType for u16 {
    type Target = MysqlTypeInt4Marker;

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

impl MysqlType for u32 {
    type Target = MysqlTypeInt8Marker;

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

// -- Float types --------------------------------------------------------------

impl MysqlType for f32 {
    type Target = MysqlTypeFloat4Marker;

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

impl MysqlType for f64 {
    type Target = MysqlTypeFloat8Marker;

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

// -- Option<T> -> Null for None, delegates to T for Some ----------------------

impl<T: MysqlType> MysqlType for Option<T> {
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
