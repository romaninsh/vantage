use std::sync::Arc;

use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_expressions::{AnyExpression, Expression};
use vantage_types::Entity;

use crate::{
    conditions::ConditionHandle,
    pagination::Pagination,
    table::Table,
    traits::{column_like::ColumnLike, table_like::TableLike, table_source::TableSource},
};

#[async_trait]
impl<T: TableSource + 'static, E: Entity<T::Value> + 'static> TableLike for Table<T, E>
where
    T: TableSource + Send + Sync,
    E: Send + Sync,
{
    fn columns(&self) -> Arc<IndexMap<String, Arc<dyn ColumnLike>>> {
        let arc_columns: IndexMap<String, Arc<dyn ColumnLike>> = self
            .columns
            .iter()
            .map(|(k, v)| (k.clone(), Arc::new(v.clone()) as Arc<dyn ColumnLike>))
            .collect();
        Arc::new(arc_columns)
    }

    fn get_column(&self, name: &str) -> Option<Arc<dyn ColumnLike>> {
        self.columns
            .get(name)
            .map(|col| Arc::new(col.clone()) as Arc<dyn ColumnLike>)
    }

    fn table_alias(&self) -> &str {
        self.table_name()
    }

    fn table_name(&self) -> &str {
        self.table_name()
    }

    fn add_condition(&mut self, condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        // Downcast the boxed Any to Expression<T::Value>
        let expr = condition
            .downcast::<Expression<T::Value>>()
            .map_err(|_| error!("Failed to downcast condition expression"))?;

        // Add permanent condition
        let next_id = *self.next_condition_id_mut();
        let id = -next_id;
        *self.next_condition_id_mut() = next_id + 1;
        self.conditions_mut().insert(id, *expr);
        Ok(())
    }

    fn temp_add_condition(&mut self, condition: AnyExpression) -> Result<ConditionHandle> {
        // Downcast AnyExpression to Expression<T::Value>
        let expr = condition.downcast::<Expression<T::Value>>().map_err(|_| {
            error!("Failed to downcast AnyExpression to datasource expression type")
        })?;

        // Add temporary condition
        let next_id = *self.next_condition_id_mut();
        *self.next_condition_id_mut() = next_id + 1;
        self.conditions_mut().insert(next_id, expr);
        Ok(ConditionHandle::new(next_id))
    }

    fn temp_remove_condition(&mut self, handle: ConditionHandle) -> Result<()> {
        if handle.id() <= 0 {
            return Err(error!("Cannot remove permanent condition"));
        }
        self.conditions_mut().shift_remove(&handle.id());
        Ok(())
    }

    fn search_expression(&self, search_value: &str) -> Result<AnyExpression> {
        let expr = self.data_source().search_expression(self, search_value);
        Ok(AnyExpression::new(expr))
    }

    fn clone_box(&self) -> Box<dyn TableLike<Value = Self::Value, Id = Self::Id>> {
        Box::new(self.clone())
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }

    fn as_any_ref(&self) -> &dyn std::any::Any {
        self
    }

    fn set_pagination(&mut self, pagination: Option<Pagination>) {
        self.pagination = pagination;
    }

    fn get_pagination(&self) -> Option<&Pagination> {
        self.pagination.as_ref()
    }

    async fn get_count(&self) -> Result<i64> {
        self.data_source().get_count(self).await
    }

    async fn get_sum(&self, column: &dyn ColumnLike) -> Result<i64> {
        // For AnyColumn, we need to find the original column and downcast it
        let any_column = self
            .columns
            .get(column.name())
            .ok_or_else(|| error!("Column not found", column = column.name()))?;

        // This is a mock implementation - real implementations would need to handle
        // type-specific sum operations based on the column's actual type
        Ok(0)
    }

    fn title_field(&self) -> Option<Arc<dyn ColumnLike>> {
        Table::title_field(self).map(|col| Arc::new(col.clone()) as Arc<dyn ColumnLike>)
    }

    fn id_field(&self) -> Option<Arc<dyn ColumnLike>> {
        Table::id_field(self).map(|col| Arc::new(col.clone()) as Arc<dyn ColumnLike>)
    }
}
