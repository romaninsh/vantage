//! Table reference implementations for one-to-one and one-to-many relationships
//!
//! This module provides relationship management between tables, using
//! `column_table_values_expr` and `Operation::in_()` for universal backend support.

use std::any::Any;

use vantage_core::{Result, error};
use vantage_types::Entity;

use crate::{table::Table, traits::table_source::TableSource};

pub mod many;
pub mod one;

pub use many::ReferenceMany;
pub use one::ReferenceOne;

/// Trait that references between Tables must implement.
pub trait RelatedTable: Send + Sync {
    /// For a source_table return related table having necessary conditions applied
    fn get_related_table(&self, source_table: &dyn Any) -> Result<Box<dyn Any>>;

    /// Link table to current selection, making it appropriate to join both tables
    fn get_linked_table(&self, source_table: &dyn Any) -> Result<Box<dyn Any>>;

    /// Get the type name of the table this reference creates
    fn target_type_name(&self) -> &'static str;
}

/// Extension trait for RelatedTable with generic methods
pub trait RelatedTableExt {
    fn as_table<T: TableSource + 'static, E: Entity<T::Value> + 'static>(
        &self,
        source_table: &dyn Any,
    ) -> Result<Table<T, E>>;
}

impl<R: RelatedTable + ?Sized> RelatedTableExt for R {
    fn as_table<T: TableSource + 'static, E: Entity<T::Value> + 'static>(
        &self,
        source_table: &dyn Any,
    ) -> Result<Table<T, E>> {
        let any = self.get_related_table(source_table)?;
        any.downcast::<Table<T, E>>()
            .map(|boxed| *boxed)
            .map_err(|_| {
                error!(
                    "Cannot downcast to Table",
                    target_type = std::any::type_name::<T>(),
                    entity_type = std::any::type_name::<E>()
                )
            })
    }
}
