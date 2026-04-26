//! AnyRedbType extras: untyped constructor, From impls, Expressive impls.

use super::{AnyRedbType, RedbType, RedbTypeNullMarker};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

impl AnyRedbType {
    /// Create an `AnyRedbType` with no type marker. Used for values coming
    /// back from the database (or constructed by deferred resolution) where
    /// we don't have an authoritative variant yet. `try_get` on these values
    /// bypasses variant checking.
    pub fn untyped(value: ciborium::Value) -> Self {
        Self {
            value,
            type_variant: None,
        }
    }
}

/// AnyRedbType is itself a RedbType — passthrough for type-erased values.
impl RedbType for AnyRedbType {
    type Target = RedbTypeNullMarker;

    fn to_cbor(&self) -> ciborium::Value {
        self.value().clone()
    }

    fn from_cbor(value: ciborium::Value) -> Option<Self> {
        Some(AnyRedbType::untyped(value))
    }
}

/// `Option<T>` propagates the inner type's variant when `Some`, and writes
/// CBOR Null when `None`. Reading back permits both `T` and `Option<T>`.
impl<T> RedbType for Option<T>
where
    T: RedbType,
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
macro_rules! impl_from_for_redb {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for AnyRedbType {
                fn from(val: $ty) -> Self {
                    AnyRedbType::new(val)
                }
            }
        )*
    };
}

impl_from_for_redb!(i32, i64, u32, u64, f32, f64, bool, String, Vec<u8>);

impl From<&str> for AnyRedbType {
    fn from(val: &str) -> Self {
        AnyRedbType::new(val.to_string())
    }
}

// Expressive impls — let scalars flow into condition builders.
macro_rules! impl_expressive_for_redb_scalar {
    ($($ty:ty),*) => {
        $(
            impl Expressive<AnyRedbType> for $ty {
                fn expr(&self) -> Expression<AnyRedbType> {
                    Expression::new(
                        "{}",
                        vec![ExpressiveEnum::Scalar(AnyRedbType::new_ref(self))],
                    )
                }
            }
        )*
    };
}

impl_expressive_for_redb_scalar!(i32, i64, u32, u64, f32, f64, bool, String);

impl Expressive<AnyRedbType> for &str {
    fn expr(&self) -> Expression<AnyRedbType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnyRedbType::new(self.to_string()))],
        )
    }
}

impl Expressive<AnyRedbType> for AnyRedbType {
    fn expr(&self) -> Expression<AnyRedbType> {
        Expression::new("{}", vec![ExpressiveEnum::Scalar(self.clone())])
    }
}
