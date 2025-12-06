use async_trait::async_trait;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use vantage_dataset::InsertableValueSet;
use vantage_dataset::{
    ReadableValueSet, WritableValueSet,
    im::{ImDataSource, ImTable},
    prelude::VantageError,
    traits::Result,
};
use vantage_expressions::{
    Expression, expr_any,
    mocks::datasource::{MockExprDataSource, MockSelectableDataSource},
    mocks::select::MockSelect,
    traits::datasource::{DataSource, ExprDataSource, SelectableDataSource},
};
use vantage_types::{Entity, Record};

use crate::{table::Table, traits::table_like::TableLike, traits::table_source::TableSource};

#[derive(Clone)]
pub struct MockTableSource {
    data: Arc<Mutex<HashMap<String, Vec<serde_json::Value>>>>,
    im_data_source: ImDataSource,
    select_source: Option<MockSelectableDataSource>,
    query_source: Option<MockExprDataSource>,
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

    pub fn with_data(self, table_name: &str, data: Vec<serde_json::Value>) -> Self {
        self.data
            .lock()
            .unwrap()
            .insert(table_name.to_string(), data);
        self
    }

    pub async fn with_im_table(self, table_name: &str, data: Vec<serde_json::Value>) -> Self {
        let im_table = ImTable::<vantage_types::EmptyEntity>::new(&self.im_data_source, table_name);
        for value in data {
            if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
                let record = Record::from(value.clone());
                let _ = im_table
                    .replace_value(&id.to_string(), &record)
                    .await
                    .unwrap();
            }
        }
        self
    }

    pub fn with_select_source(mut self, select_source: MockSelectableDataSource) -> Self {
        self.select_source = Some(select_source);
        self
    }

    pub fn with_query_source(mut self, query_source: MockExprDataSource) -> Self {
        self.query_source = Some(query_source);
        self
    }
}

impl Default for MockTableSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for MockTableSource {}

impl ExprDataSource<serde_json::Value> for MockTableSource {
    async fn execute(
        &self,
        expr: &Expression<serde_json::Value>,
    ) -> vantage_core::Result<serde_json::Value> {
        if let Some(ref query_source) = self.query_source {
            query_source.execute(expr).await
        } else {
            panic!("MockTableSource query source not set. Use with_query_source() to configure it.")
        }
    }

    fn defer(
        &self,
        expr: Expression<serde_json::Value>,
    ) -> vantage_expressions::traits::expressive::DeferredFn<serde_json::Value>
    where
        serde_json::Value: Clone + Send + Sync + 'static,
    {
        if let Some(ref query_source) = self.query_source {
            query_source.defer(expr)
        } else {
            panic!("MockTableSource query source not set. Use with_query_source() to configure it.")
        }
    }
}

impl SelectableDataSource<serde_json::Value> for MockTableSource {
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

    async fn execute_select(
        &self,
        select: &Self::Select,
    ) -> vantage_core::Result<Vec<serde_json::Value>> {
        if let Some(ref select_source) = self.select_source {
            select_source.execute_select(select).await
        } else {
            panic!(
                "MockTableSource select source not set. Use with_select_source() to configure it."
            )
        }
    }
}

#[async_trait]
impl TableSource for MockTableSource {
    type Column = crate::column::column::Column;
    type Value = serde_json::Value;
    type Id = String;

    fn create_column(&self, name: &str, _table: impl TableLike) -> Self::Column {
        Self::Column::new(name)
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
        table: &impl TableLike,
        search_value: &str,
    ) -> Expression<Self::Value> {
        // Mock implementation: search in "name" field if it exists
        let columns = table.columns();
        if columns.contains_key("name") {
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
        match self.data.lock().unwrap().get(table.table_name()) {
            Some(data) => Ok(data.len() as i64),
            None => Ok(0),
        }
    }

    async fn get_sum<E>(&self, table: &Table<Self, E>, column: &Self::Column) -> Result<i64>
    where
        E: Entity,
        Self: Sized,
    {
        let data = self.data.lock().unwrap();
        let vec = data
            .get(table.table_name())
            .ok_or(VantageError::no_data())?;

        let mut sum = 0i64;
        for value in vec {
            if let Some(field_value) = value.get(column.name()) {
                if let Some(num) = field_value.as_i64() {
                    sum += num;
                }
            }
        }

        Ok(sum)
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
        record_with_id.insert("id".to_string(), serde_json::Value::String(id.clone()));

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
        record_with_id.insert("id".to_string(), serde_json::Value::String(id.clone()));

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
        let mock = MockTableSource::new().with_data(
            "users",
            vec![
                json!({"id": 1, "name": "Alice"}),
                json!({"id": 2, "name": "Bob"}),
            ],
        );

        let table =
            Table::<MockTableSource, TestUser>::new("users", mock).into_entity::<TestUser>();
        let count = table.data_source().get_count(&table).await.unwrap();
        assert_eq!(count, 2);
    }
}
