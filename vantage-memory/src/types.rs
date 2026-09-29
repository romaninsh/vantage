//! In-memory type system.
//!
//! `vantage-memory` keeps row bodies as CBOR directly (see `src/store.rs`);
//! `AnyMemoryType` is the typed-layer wrapper over that same representation,
//! so a `Table<MemoryDB, E>` field round-trips fully typed without going
//! through an intermediate encoding.

use vantage_core::VantageError;
use vantage_types::{Record, vantage_type_system};

vantage_type_system! {
    type_trait: MemoryType,
    method_name: cbor,
    value_type: ciborium::Value,
    null_when: ciborium::Value::Null,
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

impl MemoryTypeVariants {
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
}

mod bool;
mod bytes;
mod numbers;
mod string;
mod value;

impl std::fmt::Display for AnyMemoryType {
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

// TryFrom<AnyMemoryType> for common scalar types — used when extracting
// scalar results from queries.
macro_rules! impl_try_from_memory {
    ($($ty:ty),*) => {
        $(
            impl TryFrom<AnyMemoryType> for $ty {
                type Error = VantageError;
                fn try_from(val: AnyMemoryType) -> Result<Self, Self::Error> {
                    val.try_get::<$ty>().ok_or_else(|| {
                        vantage_core::error!(
                            "Cannot convert AnyMemoryType to target type",
                            target = std::any::type_name::<$ty>(),
                            value = format!("{}", val)
                        )
                    })
                }
            }
        )*
    };
}

impl_try_from_memory!(i32, i64, u32, u64, f32, f64, bool, String);

impl TryFrom<AnyMemoryType> for Vec<u8> {
    type Error = VantageError;
    fn try_from(val: AnyMemoryType) -> Result<Self, Self::Error> {
        val.try_get::<Vec<u8>>()
            .ok_or_else(|| vantage_core::error!("Cannot convert AnyMemoryType to Vec<u8>"))
    }
}

impl TryFrom<AnyMemoryType> for Record<AnyMemoryType> {
    type Error = VantageError;
    fn try_from(val: AnyMemoryType) -> Result<Self, Self::Error> {
        let value = val.into_value();
        match value {
            ciborium::Value::Map(pairs) => Ok(pairs
                .into_iter()
                .filter_map(|(k, v)| match k {
                    ciborium::Value::Text(s) => Some((s, AnyMemoryType::untyped(v))),
                    _ => None,
                })
                .collect()),
            _ => Err(vantage_core::error!("Expected map result")),
        }
    }
}

#[cfg(test)]
mod tests;
