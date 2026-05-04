//! DynamoDB type system.
//!
//! `AttributeValue` mirrors DynamoDB's wire shape — every value is tagged
//! by its type (S, N, B, BOOL, …). The `vantage_type_system!` macro
//! produces `DynamoType`, `AnyDynamoType`, and the variants enum on top
//! of that. Numbers go on the wire as strings (per the DynamoDB JSON
//! protocol); the per-type impls in `numbers.rs` handle the round-trip.

use vantage_core::VantageError;
use vantage_types::{Record, vantage_type_system};

mod binary;
mod bool;
mod numbers;
mod string;
mod value;

/// DynamoDB AttributeValue. The wire form is a one-key JSON object
/// (`{"S": "..."}`, `{"N": "42"}`, …); this enum is the typed
/// counterpart. Sets and nested types are listed for completeness; the
/// `DynamoType` impls cover the scalar variants in v0.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    /// String — `{"S": "value"}`
    S(String),
    /// Number — DynamoDB sends/receives these as strings to preserve precision.
    N(String),
    /// Binary — raw bytes (`{"B": "<base64>"}` on the wire).
    B(Vec<u8>),
    /// Boolean — `{"BOOL": true}`
    Bool(bool),
    /// Null — `{"NULL": true}`
    Null,
    /// List — `{"L": [...]}`
    L(Vec<AttributeValue>),
    /// Map — `{"M": {...}}`
    M(indexmap::IndexMap<String, AttributeValue>),
    /// String Set — `{"SS": [...]}`
    SS(Vec<String>),
    /// Number Set — `{"NS": [...]}`
    NS(Vec<String>),
    /// Binary Set — `{"BS": [...]}`
    BS(Vec<Vec<u8>>),
}

vantage_type_system! {
    type_trait: DynamoType,
    method_name: attr,
    value_type: AttributeValue,
    type_variants: [
        S,
        N,
        B,
        Bool,
        Null,
        L,
        M,
        SS,
        NS,
        BS,
    ]
}

impl DynamoTypeVariants {
    /// Detect the variant of an `AttributeValue` (used by the macro's
    /// `from_attr` constructor on `AnyDynamoType`).
    pub fn from_attr(value: &AttributeValue) -> Option<Self> {
        Some(match value {
            AttributeValue::S(_) => Self::S,
            AttributeValue::N(_) => Self::N,
            AttributeValue::B(_) => Self::B,
            AttributeValue::Bool(_) => Self::Bool,
            AttributeValue::Null => Self::Null,
            AttributeValue::L(_) => Self::L,
            AttributeValue::M(_) => Self::M,
            AttributeValue::SS(_) => Self::SS,
            AttributeValue::NS(_) => Self::NS,
            AttributeValue::BS(_) => Self::BS,
        })
    }
}

impl std::fmt::Display for AnyDynamoType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.value() {
            AttributeValue::S(s) => write!(f, "{:?}", s),
            AttributeValue::N(n) => write!(f, "{}", n),
            AttributeValue::B(b) => write!(f, "<{} bytes>", b.len()),
            AttributeValue::Bool(b) => write!(f, "{}", b),
            AttributeValue::Null => write!(f, "null"),
            AttributeValue::L(arr) => write!(f, "{:?}", arr),
            AttributeValue::M(map) => write!(f, "{:?}", map),
            AttributeValue::SS(s) => write!(f, "{:?}", s),
            AttributeValue::NS(s) => write!(f, "{:?}", s),
            AttributeValue::BS(s) => write!(f, "<{} binary entries>", s.len()),
        }
    }
}

// TryFrom<AnyDynamoType> for common scalar types — required by
// `AssociatedExpression::get()` (Step 2) but conceptually part of
// the type system.
macro_rules! impl_try_from_dynamo {
    ($($ty:ty),*) => {
        $(
            impl TryFrom<AnyDynamoType> for $ty {
                type Error = VantageError;
                fn try_from(val: AnyDynamoType) -> Result<Self, Self::Error> {
                    val.try_get::<$ty>().ok_or_else(|| {
                        vantage_core::error!(
                            "Cannot convert AnyDynamoType to target type",
                            target = std::any::type_name::<$ty>(),
                            value = format!("{}", val)
                        )
                    })
                }
            }
        )*
    };
}

impl_try_from_dynamo!(i32, i64, f64, bool, String);

impl TryFrom<AnyDynamoType> for Record<AnyDynamoType> {
    type Error = VantageError;
    fn try_from(val: AnyDynamoType) -> Result<Self, Self::Error> {
        match val.into_value() {
            AttributeValue::M(map) => Ok(map
                .into_iter()
                .map(|(k, v)| (k, AnyDynamoType::untyped(v)))
                .collect()),
            AttributeValue::L(arr) => {
                // Single-row result wrapped as a list — pluck the first map.
                let map = arr
                    .into_iter()
                    .find_map(|v| match v {
                        AttributeValue::M(m) => Some(m),
                        _ => None,
                    })
                    .ok_or_else(|| vantage_core::error!("Expected map in list result"))?;
                Ok(map
                    .into_iter()
                    .map(|(k, v)| (k, AnyDynamoType::untyped(v)))
                    .collect())
            }
            other => Err(vantage_core::error!(
                "Expected map or list result",
                got = format!("{:?}", other)
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_round_trip() {
        let val = AnyDynamoType::new("hello".to_string());
        assert_eq!(val.type_variant(), Some(DynamoTypeVariants::S));
        assert_eq!(val.try_get::<String>(), Some("hello".to_string()));
    }

    #[test]
    fn test_i64_round_trip() {
        let val = AnyDynamoType::new(42i64);
        assert_eq!(val.type_variant(), Some(DynamoTypeVariants::N));
        assert_eq!(val.try_get::<i64>(), Some(42));
    }

    #[test]
    fn test_bool_round_trip() {
        let val = AnyDynamoType::new(true);
        assert_eq!(val.type_variant(), Some(DynamoTypeVariants::Bool));
        assert_eq!(val.try_get::<bool>(), Some(true));
    }

    #[test]
    fn test_type_mismatch_blocked() {
        let val = AnyDynamoType::new("hello".to_string());
        assert_eq!(val.try_get::<i64>(), None);
    }

    #[test]
    fn test_untyped_permissive() {
        let val = AnyDynamoType::untyped(AttributeValue::N("42".to_string()));
        assert_eq!(val.try_get::<i64>(), Some(42));
        assert_eq!(val.try_get::<f64>(), Some(42.0));
    }

    #[test]
    fn test_option_some() {
        let val = AnyDynamoType::new(Some(42i64));
        assert_eq!(val.try_get::<Option<i64>>(), Some(Some(42)));
    }

    #[test]
    fn test_option_none() {
        let val = AnyDynamoType::new(None::<i64>);
        assert!(matches!(val.value(), AttributeValue::Null));
        assert_eq!(val.try_get::<Option<i64>>(), Some(None));
    }
}
