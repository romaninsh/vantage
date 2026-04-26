//! redb type system.
//!
//! redb is a key-value store; we store row bodies as CBOR with type-variant
//! tags so `Record<AnyRedbType>` round-trips fully typed without needing
//! the entity struct on read.

use vantage_core::VantageError;
use vantage_types::{Record, vantage_type_system};

vantage_type_system! {
    type_trait: RedbType,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [
        Null,
        Bool,
        Int,
        Float,
        String,
        Bytes,
        Array,
        Map
    ]
}

impl RedbTypeVariants {
    /// Detect a variant from a raw CBOR value (used for untyped reads).
    pub fn from_cbor(value: &ciborium::Value) -> Option<Self> {
        use ciborium::Value::*;
        match value {
            Null => Some(Self::Null),
            Bool(_) => Some(Self::Bool),
            Integer(_) => Some(Self::Int),
            Float(_) => Some(Self::Float),
            Text(_) => Some(Self::String),
            Bytes(_) => Some(Self::Bytes),
            Array(_) => Some(Self::Array),
            Map(_) => Some(Self::Map),
            Tag(_, inner) => Self::from_cbor(inner),
            _ => None,
        }
    }

    /// Stable index used in the on-disk row encoding.
    pub fn to_index(self) -> u8 {
        match self {
            Self::Null => 0,
            Self::Bool => 1,
            Self::Int => 2,
            Self::Float => 3,
            Self::String => 4,
            Self::Bytes => 5,
            Self::Array => 6,
            Self::Map => 7,
        }
    }

    pub fn from_index(idx: u8) -> Option<Self> {
        match idx {
            0 => Some(Self::Null),
            1 => Some(Self::Bool),
            2 => Some(Self::Int),
            3 => Some(Self::Float),
            4 => Some(Self::String),
            5 => Some(Self::Bytes),
            6 => Some(Self::Array),
            7 => Some(Self::Map),
            _ => None,
        }
    }
}

mod bool;
mod bytes;
mod numbers;
mod serial;
mod string;
mod value;

pub use serial::{decode_record, encode_record, encode_value, value_to_index_key};

impl std::fmt::Display for AnyRedbType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.value {
            ciborium::Value::Null => write!(f, "null"),
            ciborium::Value::Bool(b) => write!(f, "{}", b),
            ciborium::Value::Integer(i) => write!(f, "{:?}", i),
            ciborium::Value::Float(x) => write!(f, "{}", x),
            ciborium::Value::Text(s) => write!(f, "{:?}", s),
            ciborium::Value::Bytes(b) => write!(f, "Bytes({} bytes)", b.len()),
            other => write!(f, "{:?}", other),
        }
    }
}

// TryFrom<AnyRedbType> for common scalar types — used when extracting
// scalar results from queries.
macro_rules! impl_try_from_redb {
    ($($ty:ty),*) => {
        $(
            impl TryFrom<AnyRedbType> for $ty {
                type Error = VantageError;
                fn try_from(val: AnyRedbType) -> Result<Self, Self::Error> {
                    val.try_get::<$ty>().ok_or_else(|| {
                        vantage_core::error!(
                            "Cannot convert AnyRedbType to target type",
                            target = std::any::type_name::<$ty>(),
                            value = format!("{}", val)
                        )
                    })
                }
            }
        )*
    };
}

impl_try_from_redb!(i32, i64, u32, u64, f32, f64, bool, String);

impl TryFrom<AnyRedbType> for Vec<u8> {
    type Error = VantageError;
    fn try_from(val: AnyRedbType) -> Result<Self, Self::Error> {
        val.try_get::<Vec<u8>>()
            .ok_or_else(|| vantage_core::error!("Cannot convert AnyRedbType to Vec<u8>"))
    }
}

impl TryFrom<AnyRedbType> for Record<AnyRedbType> {
    type Error = VantageError;
    fn try_from(val: AnyRedbType) -> Result<Self, Self::Error> {
        let value = val.into_value();
        match value {
            ciborium::Value::Map(pairs) => Ok(pairs
                .into_iter()
                .filter_map(|(k, v)| match k {
                    ciborium::Value::Text(s) => Some((s, AnyRedbType::untyped(v))),
                    _ => None,
                })
                .collect()),
            _ => Err(vantage_core::error!("Expected map result")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int_round_trip() {
        let v = AnyRedbType::new(42i64);
        assert_eq!(v.type_variant(), Some(RedbTypeVariants::Int));
        assert_eq!(v.try_get::<i64>(), Some(42));
        assert_eq!(v.try_get::<String>(), None);
    }

    #[test]
    fn test_string_round_trip() {
        let v = AnyRedbType::new("hello".to_string());
        assert_eq!(v.type_variant(), Some(RedbTypeVariants::String));
        assert_eq!(v.try_get::<String>(), Some("hello".to_string()));
        assert_eq!(v.try_get::<i64>(), None);
    }

    #[test]
    fn test_bool_round_trip() {
        let v = AnyRedbType::new(true);
        assert_eq!(v.type_variant(), Some(RedbTypeVariants::Bool));
        assert_eq!(v.try_get::<bool>(), Some(true));
    }

    #[test]
    fn test_float_round_trip() {
        let v = AnyRedbType::new(2.71f64);
        assert_eq!(v.type_variant(), Some(RedbTypeVariants::Float));
        assert_eq!(v.try_get::<f64>(), Some(2.71));
    }

    #[test]
    fn test_untyped_permissive() {
        let v = AnyRedbType::untyped(ciborium::Value::Integer(42i64.into()));
        assert_eq!(v.type_variant(), None);
        assert_eq!(v.try_get::<i64>(), Some(42));
    }
}
