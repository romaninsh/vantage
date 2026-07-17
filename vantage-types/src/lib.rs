// Re-export proc-macros from entity subcrate
pub use vantage_types_entity::entity;

// Include type_system module with regular macros
#[cfg(feature = "serde")]
pub mod cbor_json;
pub mod null;
pub mod prelude;
pub mod record;
pub mod terminal_render;
pub mod type_system;

#[cfg(feature = "serde")]
pub use cbor_json::{
    CborDialect, PlainDialect, PresentationDialect, cbor_to_json, cbor_to_string, json_to_cbor,
};
pub use null::InvariantValue;
pub use record::{IntoRecord, Record, TryFromRecord, TryIntoRecord};
pub use terminal_render::{RichText, Span, Style, TerminalRender};

/// Empty entity type for testing and dynamic table scenarios
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmptyEntity;

/// Entity trait with conversion requirements and default Value type
///
/// This trait combines the Entity requirements from vantage-core with
/// the conversion traits needed for Record operations. It defaults to
/// serde_json::Value as the value type for convenience.
#[cfg(feature = "serde")]
pub trait Entity<Value: Clone = serde_json::Value>:
    TryIntoRecord<Value> + TryFromRecord<Value> + Send + Sync + Clone
{
}

// Blanket implementation for any type that satisfies the bounds
#[cfg(feature = "serde")]
impl<T, Value> Entity<Value> for T
where
    Value: Clone,
    T: TryIntoRecord<Value> + TryFromRecord<Value> + Send + Sync + Clone,
{
}

/// Entity trait without serde feature - no default Value type
#[cfg(not(feature = "serde"))]
pub trait Entity<Value: Clone>:
    TryIntoRecord<Value> + TryFromRecord<Value> + Send + Sync + Clone
{
}

// Blanket implementation for any type that satisfies the bounds
#[cfg(not(feature = "serde"))]
impl<T, Value> Entity<Value> for T
where
    Value: Clone,
    T: TryIntoRecord<Value> + TryFromRecord<Value> + Send + Sync + Clone,
{
}

// Implement conversion traits for EmptyEntity with any value type
impl<V: Clone> IntoRecord<V> for EmptyEntity {
    fn into_record(self) -> Record<V> {
        Record::new()
    }
}

impl<V: Clone> TryIntoRecord<V> for EmptyEntity {
    type Error = std::convert::Infallible; // No conversion can fail for empty entity

    fn try_into_record(self) -> Result<Record<V>, Self::Error> {
        Ok(Record::new())
    }
}

impl<V: Clone> TryFromRecord<V> for EmptyEntity {
    type Error = (); // No conversion can fail for empty entity

    fn from_record(_record: Record<V>) -> Result<Self, Self::Error> {
        Ok(EmptyEntity)
    }
}
