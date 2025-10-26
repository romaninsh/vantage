//! Table reference implementations for one-to-one and one-to-many relationships
//!
//! This module provides relationship management between tables, similar to 0.2 but
//! using the new 0.3 architecture with AnyTable and generic TableSource.

use std::marker::PhantomData;
use std::sync::Arc;

use crate::{ColumnLike, Entity, Table, TableSource, any::AnyTable};
use vantage_core::{Result, error, util::error::Context};
use vantage_expressions::{IntoExpressive, protocol::selectable::Selectable};

/// Trait for applying relationship conditions to tables
/// Works with concrete Table types to enable proper condition application
pub trait RelatedTable: Send + Sync {
    /// Get related table with conditions for fetching related records
    /// Uses IN (SELECT ...) subquery pattern
    fn get_related_table(&self, source_table: &dyn std::any::Any) -> Result<AnyTable>;

    /// Get linked table with conditions for JOINs/subqueries
    /// Uses direct column equality pattern
    fn get_linked_table(&self, source_table: &dyn std::any::Any) -> Result<AnyTable>;
}

/// One-to-one relationship reference
///
/// Example: Client has one Bakery (via bakery_id)
/// - foreign_key: "bakery_id" (column on Client table)
/// - get_table: returns Table<SurrealDB, Bakery>
pub struct ReferenceOne<T: TableSource, SourceE: Entity, TargetE: Entity> {
    /// Foreign key column name on the source table
    our_foreign_key: String,
    /// Factory function that creates the target table
    get_table: Arc<dyn Fn() -> Table<T, TargetE> + Send + Sync>,
    _phantom: PhantomData<(T, SourceE, TargetE)>,
}

impl<T: TableSource + 'static, SourceE: Entity + 'static, TargetE: Entity + 'static>
    ReferenceOne<T, SourceE, TargetE>
{
    /// Create a new one-to-one reference
    ///
    /// # Arguments
    /// * `our_foreign_key` - Column name on source table (e.g., "bakery_id")
    /// * `get_table` - Closure that returns the target table
    pub fn new(
        our_foreign_key: impl Into<String>,
        get_table: impl Fn() -> Table<T, TargetE> + Send + Sync + 'static,
    ) -> Self {
        Self {
            our_foreign_key: our_foreign_key.into(),
            get_table: Arc::new(get_table),
            _phantom: PhantomData,
        }
    }
}

impl<T: TableSource, SourceE: Entity, TargetE: Entity> std::fmt::Debug
    for ReferenceOne<T, SourceE, TargetE>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReferenceOne")
            .field("our_foreign_key", &self.our_foreign_key)
            .finish()
    }
}

impl<T: TableSource, SourceE: Entity, TargetE: Entity> Clone for ReferenceOne<T, SourceE, TargetE> {
    fn clone(&self) -> Self {
        Self {
            our_foreign_key: self.our_foreign_key.clone(),
            get_table: self.get_table.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<T: TableSource + 'static, SourceE: Entity + 'static, TargetE: Entity + 'static> RelatedTable
    for ReferenceOne<T, SourceE, TargetE>
where
    T::Column: ColumnLike,
    T: vantage_expressions::SelectSource<T::Expr>,
    T::Select<SourceE>: Into<T::Expr>,
{
    fn get_related_table(&self, source_table: &dyn std::any::Any) -> Result<AnyTable> {
        let source = source_table
            .downcast_ref::<Table<T, SourceE>>()
            .ok_or_else(|| error!("Source table type mismatch in ReferenceOne"))?;
        let mut target = (self.get_table)();

        let target_id = target
            .column("id")
            .ok_or_else(|| error!("Target table must have 'id' column"))?;
        let source_fk = source.column(&self.our_foreign_key).ok_or_else(|| {
            error!(
                "Foreign key not found on source table",
                column = self.our_foreign_key.as_str()
            )
        })?;

        // Build subquery: SELECT fk FROM source WHERE <source conditions>
        let mut subquery = source.select();
        subquery.clear_fields();
        subquery.add_field(source_fk.name());

        let condition = target.data_source().expr(
            format!("{} IN ({})", target_id.name(), "{}"),
            vec![IntoExpressive::nested(subquery.into())],
        );
        target.add_condition(condition);
        Ok(AnyTable::new(target))
    }

    fn get_linked_table(&self, source_table: &dyn std::any::Any) -> Result<AnyTable> {
        let source = source_table
            .downcast_ref::<Table<T, SourceE>>()
            .ok_or_else(|| error!("Source table type mismatch in ReferenceOne"))?;
        let mut target = (self.get_table)();

        let target_id = target
            .column("id")
            .ok_or_else(|| error!("Target table must have 'id' column"))?;
        let source_fk = source.column(&self.our_foreign_key).ok_or_else(|| {
            error!(
                "Foreign key not found on source table",
                column = self.our_foreign_key.as_str()
            )
        })?;

        let condition = target.data_source().expr(
            format!(
                "{}.{} = {}.{}",
                target.table_name(),
                target_id.name(),
                source.table_name(),
                source_fk.name()
            ),
            vec![],
        );
        target.add_condition(condition);
        Ok(AnyTable::new(target))
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

impl<T: TableSource + 'static, SourceE: Entity + 'static, TargetE: Entity + 'static>
    ReferenceMany<T, SourceE, TargetE>
{
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

impl<T: TableSource + 'static, SourceE: Entity + 'static, TargetE: Entity + 'static> RelatedTable
    for ReferenceMany<T, SourceE, TargetE>
where
    T::Column: ColumnLike,
    T: vantage_expressions::SelectSource<T::Expr>,
    T::Select<SourceE>: Into<T::Expr>,
{
    fn get_related_table(&self, source_table: &dyn std::any::Any) -> Result<AnyTable> {
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
            vec![IntoExpressive::nested(subquery.into())],
        );
        target.add_condition(condition);
        Ok(AnyTable::new(target))
    }

    fn get_linked_table(&self, source_table: &dyn std::any::Any) -> Result<AnyTable> {
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
        Ok(AnyTable::new(target))
    }
}

// Tests disabled - MockTableSource doesn't implement SelectSource
// Reference functionality is tested via bakery_model4 CLI examples
