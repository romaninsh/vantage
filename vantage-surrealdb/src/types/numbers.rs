//! Numeric type implementations for SurrealDB
//!
//! This module provides implementations of the SurrealType trait for all standard Rust numeric types.

use crate::types::{SurrealType, SurrealTypeFloatMarker, SurrealTypeIntMarker};
use ciborium::{Value as CborValue, value::Integer};

// Signed integers
impl SurrealType for i8 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => i8::try_from(i).ok(),
            _ => None,
        }
    }
}

impl SurrealType for i16 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => i16::try_from(i).ok(),
            _ => None,
        }
    }
}

impl SurrealType for i32 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => i32::try_from(i).ok(),
            _ => None,
        }
    }
}

impl SurrealType for i64 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => i64::try_from(i).ok(),
            _ => None,
        }
    }
}

impl SurrealType for isize {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => isize::try_from(i).ok(),
            _ => None,
        }
    }
}

// Unsigned integers
impl SurrealType for u8 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => u8::try_from(i).ok(),
            _ => None,
        }
    }
}

impl SurrealType for u16 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => u16::try_from(i).ok(),
            _ => None,
        }
    }
}

impl SurrealType for u32 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => u32::try_from(i).ok(),
            _ => None,
        }
    }
}

impl SurrealType for u64 {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => u64::try_from(i).ok(),
            _ => None,
        }
    }
}

impl SurrealType for usize {
    type Target = SurrealTypeIntMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Integer((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Integer(i) => usize::try_from(i).ok(),
            _ => None,
        }
    }
}

// Floating point numbers
impl SurrealType for f32 {
    type Target = SurrealTypeFloatMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Float((*self).into())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Float(f) => Some(f as f32),
            _ => None,
        }
    }
}

impl SurrealType for f64 {
    type Target = SurrealTypeFloatMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Float(*self)
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Float(f) => Some(f),
            _ => None,
        }
    }
}
