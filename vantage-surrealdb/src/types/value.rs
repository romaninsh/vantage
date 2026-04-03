//! Value type implementations for SurrealDB
//!
//! AnySurrealType passthrough and conversions.

use crate::types::{AnySurrealType, SurrealType, SurrealTypeNoneMarker};
use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
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

impl From<JsonValue> for AnySurrealType {
    fn from(val: JsonValue) -> Self {
        let cbor = json_to_cbor(val);
        AnySurrealType::from_cbor(&cbor).expect("json_to_cbor produced unconvertible CBOR")
    }
}

fn json_to_cbor(val: JsonValue) -> CborValue {
    match val {
        JsonValue::Null => CborValue::Null,
        JsonValue::Bool(b) => CborValue::Bool(b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                CborValue::Integer(i.into())
            } else if let Some(f) = n.as_f64() {
                CborValue::Float(f)
            } else {
                CborValue::Text(n.to_string())
            }
        }
        JsonValue::String(s) => CborValue::Text(s),
        JsonValue::Array(arr) => CborValue::Array(arr.into_iter().map(json_to_cbor).collect()),
        JsonValue::Object(map) => CborValue::Map(
            map.into_iter()
                .map(|(k, v)| (CborValue::Text(k), json_to_cbor(v)))
                .collect(),
        ),
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
