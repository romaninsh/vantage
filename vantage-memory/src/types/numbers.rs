//! Numeric type implementations for the in-memory store.

use super::{MemoryType, MemoryTypeFloatMarker, MemoryTypeIntMarker};
use ciborium::Value as CborValue;

macro_rules! impl_int {
    ($($ty:ty),*) => {
        $(
            impl MemoryType for $ty {
                type Target = MemoryTypeIntMarker;

                fn to_cbor(&self) -> CborValue {
                    CborValue::Integer((*self).into())
                }

                fn from_cbor(cbor: CborValue) -> Option<Self> {
                    match cbor {
                        CborValue::Integer(i) => <$ty>::try_from(i).ok(),
                        _ => None,
                    }
                }
            }
        )*
    };
}

impl_int!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl MemoryType for f32 {
    type Target = MemoryTypeFloatMarker;

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

impl MemoryType for f64 {
    type Target = MemoryTypeFloatMarker;

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
