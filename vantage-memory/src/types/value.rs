//! AnyMemoryType extras: untyped constructor, From impls, Expressive impls.

use super::{AnyMemoryType, MemoryType, MemoryTypeNullMarker};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

impl AnyMemoryType {
    /// Create an `AnyMemoryType` with no type marker. Used for values coming
    /// back from the store (or constructed by deferred resolution) where we
    /// don't have an authoritative variant yet. `try_get` on these values
    /// bypasses variant checking.
    pub fn untyped(value: ciborium::Value) -> Self {
        Self {
            value,
            type_variant: None,
        }
    }
}

/// AnyMemoryType is itself a MemoryType — passthrough for type-erased values.
impl MemoryType for AnyMemoryType {
    type Target = MemoryTypeNullMarker;

    fn to_cbor(&self) -> ciborium::Value {
        self.value().clone()
    }

    fn from_cbor(value: ciborium::Value) -> Option<Self> {
        Some(AnyMemoryType::untyped(value))
    }
}

/// `Option<T>` propagates the inner type's variant when `Some`, and writes
/// CBOR Null when `None`. Reading back permits both `T` and `Option<T>`.
impl<T> MemoryType for Option<T>
where
    T: MemoryType,
{
    type Target = T::Target;

    fn to_cbor(&self) -> ciborium::Value {
        match self {
            Some(v) => v.to_cbor(),
            None => ciborium::Value::Null,
        }
    }

    fn from_cbor(value: ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Null => Some(None),
            other => T::from_cbor(other).map(Some),
        }
    }
}

// From impls for common types
macro_rules! impl_from_for_memory {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for AnyMemoryType {
                fn from(val: $ty) -> Self {
                    AnyMemoryType::new(val)
                }
            }
        )*
    };
}

impl_from_for_memory!(i32, i64, u32, u64, f32, f64, bool, String, Vec<u8>);

impl From<&str> for AnyMemoryType {
    fn from(val: &str) -> Self {
        AnyMemoryType::new(val.to_string())
    }
}

// Expressive impls — let scalars flow into condition builders.
macro_rules! impl_expressive_for_memory_scalar {
    ($($ty:ty),*) => {
        $(
            impl Expressive<AnyMemoryType> for $ty {
                fn expr(&self) -> Expression<AnyMemoryType> {
                    Expression::new(
                        "{}",
                        vec![ExpressiveEnum::Scalar(AnyMemoryType::new_ref(self))],
                    )
                }
            }
        )*
    };
}

impl_expressive_for_memory_scalar!(i32, i64, u32, u64, f32, f64, bool, String);

impl Expressive<AnyMemoryType> for &str {
    fn expr(&self) -> Expression<AnyMemoryType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnyMemoryType::new(self.to_string()))],
        )
    }
}

impl Expressive<AnyMemoryType> for AnyMemoryType {
    fn expr(&self) -> Expression<AnyMemoryType> {
        Expression::new("{}", vec![ExpressiveEnum::Scalar(self.clone())])
    }
}
