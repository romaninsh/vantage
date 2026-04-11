//! Table reference system for relationships between tables.
//!
//! The `Reference` trait describes a relationship (field names, target factory).
//! Resolution (building conditions) happens in `Table::get_ref_as`.
//!
//! Three concrete types:
//! - `HasOne` — foreign key on source table (e.g. Client.bakery_id → Bakery)
//! - `HasMany` — foreign key on target table (e.g. Bakery → Client.bakery_id)
//! - `HasForeign` — cross-persistence reference with user-provided resolution

use std::any::Any;

use vantage_core::Result;

use crate::any::AnyTable;

pub mod foreign;
pub mod many;
pub mod one;

pub use foreign::HasForeign;
pub use many::HasMany;
pub use one::HasOne;

/// Describes a relationship between two tables.
pub trait Reference: Send + Sync {
    /// Given source and target id field names, return (source_column, target_column).
    fn columns(&self, source_id: &str, target_id: &str) -> (String, String);

    /// Produce a fresh target table (no conditions applied).
    fn build_target(&self, data_source: &dyn Any) -> Box<dyn Any>;

    /// Whether this is a cross-persistence reference.
    fn is_foreign(&self) -> bool {
        false
    }

    /// Resolve this reference and return an AnyTable.
    ///
    /// For same-backend: builds target, applies condition, wraps in AnyTable.
    /// For foreign: returns the AnyTable from the user's closure.
    fn resolve_as_any(&self, source_table: &dyn Any) -> Result<AnyTable>;

    /// Type name of the target table (for error messages).
    fn target_type_name(&self) -> &'static str;
}
