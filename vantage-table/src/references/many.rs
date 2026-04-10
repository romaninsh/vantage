use std::{any::Any, marker::PhantomData, sync::Arc};

use vantage_core::{Result, error};
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::Expression;
use vantage_types::Entity;

use crate::{
    references::RelatedTable, table::Table, traits::column_like::ColumnLike,
    traits::table_source::TableSource,
};

/// One-to-many relationship reference
///
/// Example: Bakery has many Clients (via client.bakery_id)
/// - foreign_key: "bakery_id" (column on Client table pointing to Bakery)
/// - get_table: returns Table<SurrealDB, Client>
pub struct ReferenceMany<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> {
    /// Foreign key column name on the target table
    target_foreign_key: String,
    /// Factory function that creates the target table
    get_table: Arc<dyn Fn() -> Table<T, TargetE> + Send + Sync>,
    _phantom: PhantomData<(T, SourceE, TargetE)>,
}

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>>
    ReferenceMany<T, SourceE, TargetE>
{
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

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> std::fmt::Debug
    for ReferenceMany<T, SourceE, TargetE>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReferenceMany")
            .field("target_foreign_key", &self.target_foreign_key)
            .finish()
    }
}

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> Clone
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

impl<T: TableSource, SourceE: Entity<T::Value> + 'static, TargetE: Entity<T::Value> + 'static>
    RelatedTable for ReferenceMany<T, SourceE, TargetE>
where
    T: ExprDataSource<T::Value>,
    T::Value: Clone + Send + Sync + 'static,
    T::Condition: From<Expression<T::Value>>,
{
    fn get_related_table(&self, source_table: &dyn Any) -> Result<Box<dyn Any>> {
        let source = source_table
            .downcast_ref::<Table<T, SourceE>>()
            .ok_or_else(|| error!("Source table type mismatch in ReferenceMany"))?;
        let mut target = (self.get_table)();

        let source_id = source
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let condition = target.data_source().related_in_condition(
            &self.target_foreign_key,
            source,
            &source_id,
        );
        target.add_condition(condition);
        Ok(Box::new(target))
    }

    fn get_linked_table(&self, source_table: &dyn Any) -> Result<Box<dyn Any>> {
        let source = source_table
            .downcast_ref::<Table<T, SourceE>>()
            .ok_or_else(|| error!("Source table type mismatch in ReferenceMany"))?;
        let mut target = (self.get_table)();

        let condition = target.data_source().expr(
            format!(
                "{}.{} = {}.id",
                target.table_name(),
                self.target_foreign_key,
                source.table_name()
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
