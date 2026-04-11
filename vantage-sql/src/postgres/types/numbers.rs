//! Numeric type implementations for PostgreSQL.
//!
//! Uses ciborium::Value (CBOR) as the underlying storage:
//! - Integers → CborValue::Integer
//! - Floats → CborValue::Float
//! - Option<T> delegates to T, with Null for None

use super::{
    PostgresType, PostgresTypeFloat4Marker, PostgresTypeFloat8Marker, PostgresTypeInt2Marker,
    PostgresTypeInt4Marker, PostgresTypeInt8Marker,
};
use ciborium::Value;

// -- i16 -> Int2 (SMALLINT) ---------------------------------------------------

impl PostgresType for i16 {
    type Target = PostgresTypeInt2Marker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => i16::try_from(i).ok(),
            _ => None,
        }
    }
}

// -- i32 -> Int4 (INTEGER) ----------------------------------------------------

impl PostgresType for i32 {
    type Target = PostgresTypeInt4Marker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => i32::try_from(i).ok(),
            _ => None,
        }
    }
}

// -- i64 -> Int8 (BIGINT) ----------------------------------------------------

impl PostgresType for i64 {
    type Target = PostgresTypeInt8Marker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => i64::try_from(i).ok(),
            _ => None,
        }
    }
}

// -- Smaller integer types mapped to appropriate PostgreSQL variants -----------

impl PostgresType for i8 {
    type Target = PostgresTypeInt2Marker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => i8::try_from(i).ok(),
            _ => None,
        }
    }
}

impl PostgresType for u8 {
    type Target = PostgresTypeInt2Marker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => u8::try_from(i).ok(),
            _ => None,
        }
    }
}

impl PostgresType for u16 {
    type Target = PostgresTypeInt4Marker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => u16::try_from(i).ok(),
            _ => None,
        }
    }
}

impl PostgresType for u32 {
    type Target = PostgresTypeInt8Marker;

    fn to_cbor(&self) -> Value {
        Value::Integer((*self).into())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Integer(i) => u32::try_from(i).ok(),
            _ => None,
        }
    }
}

// -- Float types --------------------------------------------------------------

impl PostgresType for f32 {
    type Target = PostgresTypeFloat4Marker;

    fn to_cbor(&self) -> Value {
        Value::Float((*self) as f64)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Float(f) => Some(f as f32),
            Value::Integer(i) => i64::try_from(i).ok().map(|n| n as f32),
            _ => None,
        }
    }
}

impl PostgresType for f64 {
    type Target = PostgresTypeFloat8Marker;

    fn to_cbor(&self) -> Value {
        Value::Float(*self)
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Float(f) => Some(f),
            Value::Integer(i) => i64::try_from(i).ok().map(|n| n as f64),
            _ => None,
        }
    }
}

// -- Option<T> -> Null for None, delegates to T for Some ----------------------

impl<T: PostgresType> PostgresType for Option<T> {
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
