use std::{any::Any, marker::PhantomData, sync::Arc};

use vantage_core::{Result, error};
use vantage_expressions::Expressive;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_types::Entity;

use crate::{
    operation::Operation,
    references::RelatedTable,
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

/// One-to-one relationship reference
///
/// Example: Client has one Bakery (via bakery_id)
/// - foreign_key: "bakery_id" (column on Client table)
/// - get_table: returns Table<SurrealDB, Bakery>
pub struct ReferenceOne<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> {
    /// Foreign key column name on the source table
    our_foreign_key: String,
    /// Factory function that creates the target table
    get_table: Arc<dyn Fn() -> Table<T, TargetE> + Send + Sync>,
    _phantom: PhantomData<(T, SourceE, TargetE)>,
}

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>>
    ReferenceOne<T, SourceE, TargetE>
{
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

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> std::fmt::Debug
    for ReferenceOne<T, SourceE, TargetE>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReferenceOne")
            .field("our_foreign_key", &self.our_foreign_key)
            .finish()
    }
}

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> Clone
    for ReferenceOne<T, SourceE, TargetE>
{
    fn clone(&self) -> Self {
        Self {
            our_foreign_key: self.our_foreign_key.clone(),
            get_table: self.get_table.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<T: TableSource, SourceE: Entity<T::Value> + 'static, TargetE: Entity<T::Value> + 'static>
    RelatedTable for ReferenceOne<T, SourceE, TargetE>
where
    T: ExprDataSource<T::Value>,
    T::Value: Clone + Send + Sync + 'static,
    T::Column<T::AnyType>: Operation<T::Value>,
{
    fn get_related_table(&self, source_table: &dyn Any) -> Result<Box<dyn Any>> {
        let source = source_table
            .downcast_ref::<Table<T, SourceE>>()
            .ok_or_else(|| error!("Source table type mismatch in ReferenceOne"))?;
        let mut target = (self.get_table)();

        let fk_col = source
            .data_source()
            .create_column::<T::AnyType>(&self.our_foreign_key);
        let fk_values = source
            .data_source()
            .column_values_expression(source, &fk_col);

        let id_field = target
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        target.add_condition(target[id_field.as_str()].in_(fk_values.expr()));
        Ok(Box::new(target))
    }

    fn get_linked_table(&self, source_table: &dyn Any) -> Result<Box<dyn Any>> {
        let source = source_table
            .downcast_ref::<Table<T, SourceE>>()
            .ok_or_else(|| error!("Source table type mismatch in ReferenceOne"))?;
        let mut target = (self.get_table)();

        let id_field = target
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let condition = target.data_source().expr(
            format!(
                "{}.{} = {}.{}",
                target.table_name(),
                id_field,
                source.table_name(),
                self.our_foreign_key
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
