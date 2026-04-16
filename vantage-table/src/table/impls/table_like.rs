use async_trait::async_trait;
use vantage_core::{Result, error};
use vantage_expressions::AnyExpression;
use vantage_types::Entity;

use crate::{
    conditions::ConditionHandle,
    pagination::Pagination,
    table::Table,
    traits::{table_like::TableLike, table_source::TableSource},
};

#[async_trait]
impl<T: TableSource + 'static, E: Entity<T::Value> + 'static> TableLike for Table<T, E>
where
    T: TableSource + Send + Sync,
    E: Send + Sync,
{
    fn table_alias(&self) -> &str {
        self.table_name()
    }

    fn table_name(&self) -> &str {
        self.table_name()
    }

    fn column_names(&self) -> Vec<String> {
        self.columns.keys().cloned().collect()
    }

    fn add_condition(&mut self, condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        // Downcast the boxed Any to T::Condition
        let cond = condition
            .downcast::<T::Condition>()
            .map_err(|_| error!("Failed to downcast condition to datasource condition type"))?;

        // Add permanent condition
        let next_id = *self.next_condition_id_mut();
        let id = -next_id;
        *self.next_condition_id_mut() = next_id + 1;
        self.conditions_mut().insert(id, *cond);
        Ok(())
    }

    fn temp_add_condition(&mut self, condition: AnyExpression) -> Result<ConditionHandle> {
        // Downcast AnyExpression to T::Condition
        let cond = condition
            .downcast::<T::Condition>()
            .map_err(|_| error!("Failed to downcast AnyExpression to datasource condition type"))?;

        // Add temporary condition
        let next_id = *self.next_condition_id_mut();
        *self.next_condition_id_mut() = next_id + 1;
        self.conditions_mut().insert(next_id, cond);
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
        let cond = self
            .data_source()
            .search_table_condition(self, search_value);
        Ok(AnyExpression::new(cond))
    }

    fn clone_box(&self) -> Box<dyn TableLike<Value = Self::Value, Id = Self::Id>> {
        Box::new((*self).clone())
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
        self.data_source().get_table_count(self).await
    }
}
