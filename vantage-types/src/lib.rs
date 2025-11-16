// Re-export proc-macros from persistence subcrate
pub use vantage_types_persistence::{persistence, persistence_serde};

// Include type_system module with regular macros
pub mod record;
pub mod type_system;

// Re-export Record type at crate root
pub use record::Record;

/// Entity trait with conversion requirements and default Value type
///
/// This trait combines the Entity requirements from vantage-core with
/// the conversion traits needed for Record operations. It defaults to
/// serde_json::Value as the value type for convenience.
#[cfg(feature = "serde")]
pub trait Entity<Value = serde_json::Value>:
    Into<Record<Value>> + TryFrom<Record<Value>> + Send + Sync + Clone
{
}

// Blanket implementation for any type that satisfies the bounds
#[cfg(feature = "serde")]
impl<T, Value> Entity<Value> for T where
    T: Into<Record<Value>> + TryFrom<Record<Value>> + Send + Sync + Clone
{
}
