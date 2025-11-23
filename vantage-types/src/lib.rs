// Re-export proc-macros from entity subcrate
pub use vantage_types_entity::entity;

// Include type_system module with regular macros
pub mod prelude;
pub mod record;
pub mod type_system;

pub use record::{IntoRecord, Record, TryFromRecord};

/// Entity trait with conversion requirements and default Value type
///
/// This trait combines the Entity requirements from vantage-core with
/// the conversion traits needed for Record operations. It defaults to
/// serde_json::Value as the value type for convenience.
#[cfg(feature = "serde")]
pub trait Entity<Value: Clone = serde_json::Value>:
    IntoRecord<Value> + TryFromRecord<Value> + Send + Sync + Clone
{
}

// Blanket implementation for any type that satisfies the bounds
#[cfg(feature = "serde")]
impl<T, Value> Entity<Value> for T
where
    Value: Clone,
    T: IntoRecord<Value> + TryFromRecord<Value> + Send + Sync + Clone,
{
}

/// Entity trait without serde feature - no default Value type
#[cfg(not(feature = "serde"))]
pub trait Entity<Value: Clone>:
    IntoRecord<Value> + TryFromRecord<Value> + Send + Sync + Clone
{
}

// Blanket implementation for any type that satisfies the bounds
#[cfg(not(feature = "serde"))]
impl<T, Value> Entity<Value> for T
where
    Value: Clone,
    T: IntoRecord<Value> + TryFromRecord<Value> + Send + Sync + Clone,
{
}
