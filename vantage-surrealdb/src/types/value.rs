//! Value type implementations for SurrealDB
//!
//! AnySurrealType passthrough and conversions.

use crate::types::{AnySurrealType, SurrealType, SurrealTypeNoneMarker};
use ciborium::Value as CborValue;
use vantage_expressions::{Expression, Expressive};

/// AnySurrealType implements SurrealType as a passthrough — it already holds
/// a ciborium::Value internally, so to_cbor/from_cbor just clone the inner value.
impl SurrealType for AnySurrealType {
    type Target = SurrealTypeNoneMarker;

    fn to_cbor(&self) -> CborValue {
        self.value().clone()
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        AnySurrealType::from_cbor(&cbor)
    }
}

// From impls for common types — enables `expr_param!` scalar conversion.
// Can't use blanket `From<T: SurrealType>` because it conflicts with std's `From<T> for T`.
macro_rules! impl_from_for_any {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for AnySurrealType {
                fn from(val: $ty) -> Self {
                    AnySurrealType::new(val)
                }
            }
        )*
    };
}

impl_from_for_any!(
    i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64, bool, String
);

impl From<&str> for AnySurrealType {
    fn from(val: &str) -> Self {
        AnySurrealType::new(val.to_string())
    }
}

/// Macro for explicit Expressive<AnySurrealType> impls on scalar types.
/// Lets you pass `25i64`, `true`, `"hello"` directly to RefOperation methods.
macro_rules! impl_expressive_for_scalar {
    ($($ty:ty),*) => {
        $(
            impl Expressive<AnySurrealType> for $ty {
                fn expr(&self) -> Expression<AnySurrealType> {
                    Expression::new(
                        "{}",
                        vec![vantage_expressions::ExpressiveEnum::Scalar(
                            AnySurrealType::new_ref(self),
                        )],
                    )
                }
            }
        )*
    };
}

impl_expressive_for_scalar!(
    i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64, bool, String
);

impl Expressive<AnySurrealType> for &str {
    fn expr(&self) -> Expression<AnySurrealType> {
        Expression::new(
            "{}",
            vec![vantage_expressions::ExpressiveEnum::Scalar(
                AnySurrealType::from(self.to_string()),
            )],
        )
    }
}

impl Expressive<AnySurrealType> for AnySurrealType {
    fn expr(&self) -> Expression<AnySurrealType> {
        Expression::new(
            "{}",
            vec![vantage_expressions::ExpressiveEnum::Scalar(self.clone())],
        )
    }
}
