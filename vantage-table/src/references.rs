//! Table reference implementations for one-to-one and one-to-many relationships
//!
//! This module provides relationship management between tables, similar to 0.2 but
//! using the new 0.3 architecture with AnyTable and generic TableSource.

use std::sync::Arc;
use std::{any::Any, marker::PhantomData};

use vantage_core::{Entity, Result, error};
use vantage_expressions::{Expressive, prelude::*, traits::selectable::Selectable};

use crate::{
    any::AnyTable,
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

pub mod one;

/// Trait that references between Tables must implement. It's common for a table-specific
/// trait to downcast related table reference.
pub trait RelatedTable: Send + Sync {
    /// For a source_table return related table having necessary conditions applied
    fn get_related_table(&self, source_table: &dyn Any) -> Result<Box<dyn Any>>;

    /// Link table to current selection, making it appropriate to join both tables
    fn get_linked_table(&self, source_table: &dyn Any) -> Result<Box<dyn Any>>;

    /// Try to downcast the related table to AnyTable (serde_json::Value)
    /// Alternatively you can use as_table (defined in RelatedTableExt)
    fn as_any_table(&self, source_table: &dyn Any) -> Result<AnyTable> {
        let any = self.get_related_table(source_table)?;
        any.downcast::<AnyTable>()
            .map(|boxed| *boxed)
            .map_err(|_| error!("Cannot downcast to AnyTable"))
    }

    /// Get the type name of the table this reference creates
    fn target_type_name(&self) -> &'static str;
}

/// Extension trait for RelatedTable with generic methods
pub trait RelatedTableExt {
    /// Try to downcast the related table to a specific Table<T, E> type
    fn as_table<T: TableSource + 'static, E: Entity + 'static>(
        &self,
        source_table: &dyn Any,
    ) -> Result<Table<T, E>>;
}

impl<R: RelatedTable + ?Sized> RelatedTableExt for R {
    fn as_table<T: TableSource + 'static, E: Entity + 'static>(
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

/// One-to-many relationship reference
///
/// Example: Bakery has many Clients (via client.bakery_id)
/// - foreign_key: "bakery_id" (column on Client table pointing to Bakery)
/// - get_table: returns Table<SurrealDB, Client>
pub struct ReferenceMany<T: TableSource, SourceE: Entity, TargetE: Entity> {
    /// Foreign key column name on the target table
    target_foreign_key: String,
    /// Factory function that creates the target table
    get_table: Arc<dyn Fn() -> Table<T, TargetE> + Send + Sync>,
    _phantom: PhantomData<(T, SourceE, TargetE)>,
}

impl<T: TableSource, SourceE: Entity, TargetE: Entity> ReferenceMany<T, SourceE, TargetE> {
    /// Create a new one-to-many reference
    ///
    /// # Arguments
    /// * `target_foreign_key` - Column name on target table (e.g., "bakery_id")
    /// * `get_table` - Closure that returns the target table
    pub fn new(
        target_foreign_key: impl Into<String>,
        get_table: impl Fn() -> Table<T, TargetE> + Send + Sync + 'static,
    ) -> Self {
        Self {
            target_foreign_key: target_foreign_key.into(),
            get_table: Arc::new(get_table),
            _phantom: PhantomData,
        }
    }
}

impl<T: TableSource, SourceE: Entity, TargetE: Entity> std::fmt::Debug
    for ReferenceMany<T, SourceE, TargetE>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReferenceMany")
            .field("target_foreign_key", &self.target_foreign_key)
            .finish()
    }
}

impl<T: TableSource, SourceE: Entity, TargetE: Entity> Clone
    for ReferenceMany<T, SourceE, TargetE>
{
    fn clone(&self) -> Self {
        Self {
            target_foreign_key: self.target_foreign_key.clone(),
            get_table: self.get_table.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<T: TableSource, SourceE: Entity, TargetE: Entity> RelatedTable
    for ReferenceMany<T, SourceE, TargetE>
where
    T::Column: ColumnLike,
    T: SelectSource + Expressive<T::Value>,
    T::Value: Clone + Send + Sync + 'static,
{
    fn get_related_table(&self, source_table: &dyn Any) -> Result<Box<dyn Any>> {
        let source = source_table
            .downcast_ref::<Table<T, SourceE>>()
            .ok_or_else(|| error!("Source table type mismatch in ReferenceMany"))?;
        let mut target = (self.get_table)();

        let target_fk = target.column(&self.target_foreign_key).ok_or_else(|| {
            error!(
                "Foreign key not found on target table",
                column = self.target_foreign_key.as_str()
            )
        })?;
        let source_id = source
            .column("id")
            .ok_or_else(|| error!("Source table must have 'id' column"))?;

        // Build subquery: SELECT id FROM source WHERE <source conditions>
        let mut subquery = source.select();
        subquery.clear_fields();
        subquery.add_field(source_id.name());

        let condition = target.data_source().expr(
            format!("{} IN ({})", target_fk.name(), "{}"),
            vec![Expressive::nested(subquery.into())],
        );
        target.add_condition(condition);
        Ok(Box::new(target))
    }

    fn get_linked_table(&self, source_table: &dyn Any) -> Result<Box<dyn Any>> {
        let source = source_table
            .downcast_ref::<Table<T, SourceE>>()
            .ok_or_else(|| error!("Source table type mismatch in ReferenceMany"))?;
        let mut target = (self.get_table)();

        let target_fk = target.column(&self.target_foreign_key).ok_or_else(|| {
            error!(
                "Foreign key not found on target table",
                column = self.target_foreign_key.as_str()
            )
        })?;
        let source_id = source
            .column("id")
            .ok_or_else(|| error!("Source table must have 'id' column"))?;

        let condition = target.data_source().expr(
            format!(
                "{}.{} = {}.{}",
                target.table_name(),
                target_fk.name(),
                source.table_name(),
                source_id.name()
            ),
            vec![],
        );
        target.add_condition(condition);
        Ok(Box::new(target))
    }

    fn target_type_name(&self) -> &'static str {
        std::any::type_name::<Table<T, TargetE>>()
    }
}
