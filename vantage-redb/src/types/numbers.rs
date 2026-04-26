//! Numeric type implementations for redb.

use super::{RedbType, RedbTypeFloatMarker, RedbTypeIntMarker};
use ciborium::Value as CborValue;

macro_rules! impl_int {
    ($($ty:ty),*) => {
        $(
            impl RedbType for $ty {
                type Target = RedbTypeIntMarker;

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

impl RedbType for f32 {
    type Target = RedbTypeFloatMarker;

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

impl RedbType for f64 {
    type Target = RedbTypeFloatMarker;

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
