use async_trait::async_trait;
use indexmap::IndexMap;
use rust_decimal::Decimal;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use vantage_dataset::InsertableValueSet;
use vantage_dataset::{
    ReadableValueSet, WritableValueSet,
    im::{ImDataSource, ImTable},
    prelude::VantageError,
    traits::Result,
};
use vantage_expressions::{
    Expression, expr_any,
    mocks::datasource::MockSelectableDataSource,
    mocks::select::MockSelect,
    traits::datasource::{DataSource, ExprDataSource, SelectableDataSource},
    traits::expressive::{DeferredFn, ExpressiveEnum},
};
use vantage_types::{Entity, Record};

use crate::column::column::ColumnType;
use crate::mocks::mock_column::MockColumn;
use crate::mocks::mock_type_system::AnyMockType;
use crate::traits::table_expr_source::TableExprSource;
use crate::{
    table::Table,
    traits::{column_like::ColumnLike, table_like::TableLike, table_source::TableSource},
};

#[derive(Clone)]
pub struct MockTableSource {
    data: Arc<Mutex<HashMap<String, Vec<Value>>>>,
    im_data_source: ImDataSource,
    select_source: Option<MockSelectableDataSource>,
    query_source: Option<Arc<Mutex<vantage_expressions::mocks::mock_builder::MockBuilder>>>,
}

impl MockTableSource {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            im_data_source: ImDataSource::new(),
            select_source: None,
            query_source: None,
        }
    }

    pub async fn with_data(self, table_name: &str, data: Vec<Value>) -> Self {
        // Store in HashMap for count operations
        self.data
            .lock()
            .await
            .insert(table_name.to_string(), data.clone());

        // Also store in ImDataSource for value operations
        let im_table = ImTable::<vantage_types::EmptyEntity>::new(&self.im_data_source, table_name);
        for value in data.iter() {
            if let Some(id_value) = value.get("id") {
                let id_str = match id_value {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => {
                        println!("[DEBUG] ID field is not a string or number: {:?}", id_value);
                        continue;
                    }
                };
                let record = Record::from(value.clone());
                let _ = im_table.replace_value(&id_str, &record).await;
            }
        }

        self
    }

    pub fn with_select_source(mut self, select_source: MockSelectableDataSource) -> Self {
        self.select_source = Some(select_source);
        self
    }

    pub fn with_query_source(
        mut self,
        query_source: vantage_expressions::mocks::mock_builder::MockBuilder,
    ) -> Self {
        self.query_source = Some(Arc::new(Mutex::new(query_source)));
        self
    }
}

impl Default for MockTableSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for MockTableSource {}

#[async_trait]
impl TableSource for MockTableSource {
    type Column<Type>
        = MockColumn<Type>
    where
        Type: ColumnType;
    type AnyType = AnyMockType;
    type Value = Value;
    type Id = String;

    fn create_column<Type: ColumnType>(&self, name: &str) -> Self::Column<Type> {
        use std::any::TypeId;
        let type_id = TypeId::of::<Type>();

        if type_id != TypeId::of::<String>()
            && type_id != TypeId::of::<i64>()
            && type_id != TypeId::of::<f64>()
            && type_id != TypeId::of::<Decimal>()
            && type_id != TypeId::of::<bool>()
            && type_id != TypeId::of::<Option<String>>()
            && type_id != TypeId::of::<Option<i64>>()
            && type_id != TypeId::of::<Option<f64>>()
            && type_id != TypeId::of::<Option<Decimal>>()
            && type_id != TypeId::of::<Option<bool>>()
        {
            panic!(
                "Type {:?} is not compatible with mock_type_system. Only String, i64, f64, Decimal, bool and their Optionals are supported.",
                std::any::type_name::<Type>()
            );
        }

        MockColumn::new(name)
    }

    fn to_any_column<Type: ColumnType>(
        &self,
        column: Self::Column<Type>,
    ) -> Self::Column<Self::AnyType> {
        column.into_type()
    }

    fn from_any_column<Type: ColumnType>(
        &self,
        any_column: Self::Column<Self::AnyType>,
    ) -> Option<Self::Column<Type>> {
        Some(any_column.into_type())
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<vantage_expressions::traits::expressive::ExpressiveEnum<Self::Value>>,
    ) -> Expression<Self::Value> {
        Expression::new(template, parameters)
    }

    fn search_expression(
        &self,
        _table: &impl TableLike,
        search_value: &str,
    ) -> Expression<Self::Value> {
        // Mock implementation: search in "name" field if it exists
        // Simple mock - search in name field if exists (mock implementation)
        if true {
            expr_any!("name LIKE '%{}%'", search_value)
        } else {
            panic!("Mock can only search column `name` as fulltext search")
        }
    }

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.list_values().await
    }

    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.get_value(id).await
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.get_some_value().await
    }

    async fn get_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity,
        Self: Sized,
    {
        match self.data.lock().await.get(table.table_name()) {
            Some(data) => Ok(data.len() as i64),
            None => Ok(0),
        }
    }

    async fn get_sum<E, Type: ColumnType>(
        &self,
        table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let data = self.data.lock().await;
        let _vec = data
            .get(table.table_name())
            .ok_or(VantageError::no_data())?;

        // Mock implementation - sum not supported
        Err(vantage_core::error!("Sum not implemented for MockTableSource").into())
    }

    /// Insert a record as Record value (for WritableValueSet implementation)
    async fn insert_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());

        // Check if record already exists - fail if it does
        if im_table.get_value(id).await.is_ok() {
            return Err(vantage_core::error!("Record with ID already exists", id = id).into());
        }

        let mut record_with_id = record.clone();
        record_with_id.insert("id".to_string(), Value::String(id.clone()));

        im_table.replace_value(id, &record_with_id).await
    }

    /// Replace a record as Record value (for WritableValueSet implementation)
    async fn replace_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity,
        Self: Sized,
    {
        let mut record_with_id = record.clone();
        record_with_id.insert("id".to_string(), Value::String(id.clone()));

        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.replace_value(id, &record_with_id).await
    }

    /// Patch a record as Record value (for WritableValueSet implementation)
    async fn patch_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.patch_value(id, partial).await
    }

    /// Delete a record by ID (for WritableValueSet implementation)
    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());

        // Check if record exists - fail if it doesn't
        if im_table.get_value(id).await.is_err() {
            return Err(vantage_core::error!("Record not found", id = id).into());
        }

        im_table.delete(id).await
    }

    /// Delete all records (for WritableValueSet implementation)
    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.delete_all().await
    }

    /// Insert a record and return generated ID (for InsertableValueSet implementation)
    async fn insert_table_return_id_value<E>(
        &self,
        table: &Table<Self, E>,
        record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.insert_return_id_value(record).await
    }
}

impl ExprDataSource<Value> for MockTableSource {
    async fn execute(&self, expr: &Expression<Value>) -> vantage_core::Result<Value> {
        if let Some(ref query_source) = self.query_source {
            let source = {
                let guard = query_source.lock().await;
                guard.clone()
            };
            source.execute(expr).await
        } else {
            panic!("MockTableSource query source not set. Use with_query_source() to configure it.")
        }
    }

    fn defer(
        &self,
        expr: Expression<Value>,
    ) -> vantage_expressions::traits::expressive::DeferredFn<Value>
    where
        Value: Clone + Send + Sync + 'static,
    {
        if let Some(ref query_source) = self.query_source {
            let query_source_clone = query_source.clone();
            let expr_clone = expr.clone();
            DeferredFn::new(move || {
                let query_source = query_source_clone.clone();
                let expr = expr_clone.clone();
                Box::pin(async move {
                    let source = query_source.lock().await;
                    match source.execute(&expr).await {
                        Ok(value) => Ok(ExpressiveEnum::Scalar(value)),
                        Err(e) => Err(e),
                    }
                })
            })
        } else {
            panic!("MockTableSource query source not set. Use with_query_source() to configure it.")
        }
    }
}

impl SelectableDataSource<Value> for MockTableSource {
    type Select = MockSelect;

    fn select(&self) -> Self::Select {
        if let Some(ref select_source) = self.select_source {
            select_source.select()
        } else {
            panic!(
                "MockTableSource select source not set. Use with_select_source() to configure it."
            )
        }
    }

    async fn execute_select(&self, select: &Self::Select) -> vantage_core::Result<Vec<Value>> {
        if let Some(ref select_source) = self.select_source {
            select_source.execute_select(select).await
        } else {
            panic!(
                "MockTableSource select source not set. Use with_select_source() to configure it."
            )
        }
    }
}

impl TableExprSource for MockTableSource {
    fn get_table_expr_count<E: Entity<Self::Value>>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_expressions::AssociatedExpression<'_, Self, Self::Value, usize> {
        let table_name = table.table_name();

        // Pre-calculate the count from our data
        let count = tokio::runtime::Handle::try_current()
            .map(|handle| {
                handle.block_on(async {
                    self.data
                        .lock()
                        .await
                        .get(table_name)
                        .map(|data| data.len())
                        .unwrap_or(0)
                })
            })
            .unwrap_or_else(|_| {
                // Fallback if no runtime is available
                0
            });

        // Configure the query source to return this count for the exact query
        let query_str = format!("select count() from {}", table_name);
        if let Some(ref query_source) = self.query_source {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.block_on(async {
                    let mut source = query_source.lock().await;
                    *source = source
                        .clone()
                        .on_exact_select(&query_str, serde_json::json!(count));
                });
            }
        }

        let expr = expr_any!("select count() from {}", table_name);
        vantage_expressions::AssociatedExpression::new(expr, self)
    }

    fn get_table_expr_max<E: Entity<Self::Value>, R: ColumnType>(
        &self,
        _table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> vantage_expressions::AssociatedExpression<'_, Self, Self::Value, R> {
        let column_name = column.name();
        let expr = expr_any!("select max({})", column_name);
        vantage_expressions::AssociatedExpression::new(expr, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq, Default, Clone)]
    struct TestUser {
        id: i32,
        name: String,
    }

    #[tokio::test]
    async fn test_mock_table_source_with_data() {
        let mock = MockTableSource::new()
            .with_data(
                "users",
                vec![
                    json!({"id": "1", "name": "Alice"}),
                    json!({"id": "2", "name": "Bob"}),
                ],
            )
            .await;

        let table =
            Table::<MockTableSource, TestUser>::new("users", mock).into_entity::<TestUser>();
        let count = table.data_source().get_count(&table).await.unwrap();
        assert_eq!(count, 2);
    }
}
