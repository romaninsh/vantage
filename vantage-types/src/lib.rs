//! Typed values and records: `vantage_type_system!` generates a type system,
//! `Record` carries a row of values, and `#[entity]` converts structs to and
//! from records.
//!

pub use vantage_types_entity::entity;

#[cfg(feature = "serde")]
pub mod cbor_json;
pub mod null;
pub mod prelude;
pub mod record;
pub mod terminal_render;
pub mod type_system;

#[cfg(feature = "serde")]
pub use cbor_json::{
    CborDialect, PlainDialect, PresentationDialect, cbor_id_to_string, cbor_to_json,
    cbor_to_string, hex_encode, json_to_cbor, json_to_cbor_with_hint, record_to_json,
    rfc3339_to_tag12, tag12_to_rfc3339,
};
pub use null::InvariantValue;
pub use record::{IntoRecord, Record, TryFromRecord, TryIntoRecord};
pub use terminal_render::{RichText, Span, Style, TerminalRender};

/// Empty entity type for testing and dynamic table scenarios
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmptyEntity;

/// A type convertible to and from `Record<Value>`, defaulting to
/// `serde_json::Value`. Implemented for every type that satisfies the bounds.
#[cfg(feature = "serde")]
pub trait Entity<Value: Clone = serde_json::Value>:
    TryIntoRecord<Value> + TryFromRecord<Value> + Send + Sync + Clone
{
}

#[cfg(feature = "serde")]
impl<T, Value> Entity<Value> for T
where
    Value: Clone,
    T: TryIntoRecord<Value> + TryFromRecord<Value> + Send + Sync + Clone,
{
}

/// Without the `serde` feature there is no default `Value` type.
#[cfg(not(feature = "serde"))]
pub trait Entity<Value: Clone>:
    TryIntoRecord<Value> + TryFromRecord<Value> + Send + Sync + Clone
{
}

#[cfg(not(feature = "serde"))]
impl<T, Value> Entity<Value> for T
where
    Value: Clone,
    T: TryIntoRecord<Value> + TryFromRecord<Value> + Send + Sync + Clone,
{
}

impl<V: Clone> IntoRecord<V> for EmptyEntity {
    fn into_record(self) -> Record<V> {
        Record::new()
    }
}

impl<V: Clone> TryIntoRecord<V> for EmptyEntity {
    type Error = std::convert::Infallible;

    fn try_into_record(self) -> Result<Record<V>, Self::Error> {
        Ok(Record::new())
    }
}

impl<V: Clone> TryFromRecord<V> for EmptyEntity {
    type Error = ();

    fn from_record(_record: Record<V>) -> Result<Self, Self::Error> {
        Ok(EmptyEntity)
    }
}
