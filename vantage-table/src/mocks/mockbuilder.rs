//! MockBuilder for vantage-table with TableSource support
//!
//! Wraps vantage-expressions MockBuilder and adds mockall-based TableSource
//! automocking capabilities for comprehensive table testing.

use async_trait::async_trait;
use mockall::mock;
use serde_json::Value;
use std::ops::Deref;
use vantage_core::Result;
use vantage_dataset::traits::Result as DatasetResult;
use vantage_expressions::{
    Expression,
    mocks::mockbuilder as expr_mockbuilder,
    mocks::select::MockSelect,
    traits::{
        datasource::{DataSource, QuerySource, SelectSource},
        expressive::{DeferredFn, ExpressiveEnum},
    },
};
use vantage_types::{Entity, Record};

use crate::{
    mocks::column::MockColumn,
    table::Table,
    traits::{column_like::ColumnLike, table_like::TableLike, table_source::TableSource},
};

// Create simple wrappers to avoid complex trait bounds in mock macro
#[derive(Debug, Clone)]
pub struct MockTableSourceImpl;

impl MockTableSourceImpl {
    pub fn new() -> Self {
        Self
    }

    pub fn create_column(&self, name: &str) -> MockColumn {
        MockColumn::new(name)
    }

    pub fn expr(
        &self,
        template: String,
        parameters: Vec<ExpressiveEnum<Value>>,
    ) -> Expression<Value> {
        Expression::new(template, parameters)
    }

    pub fn search_expression(&self, search_value: &str) -> Expression<Value> {
        Expression::new(
            "name LIKE '%{}%'",
            vec![ExpressiveEnum::Scalar(Value::String(
                search_value.to_string(),
            ))],
        )
    }
}

mock! {
    #[derive(Debug, Clone)]
    pub TableMethods {
        async fn get_table_data_as_value(&self, table_name: &str) -> DatasetResult<Vec<Value>>;
        async fn get_table_data_as_value_by_id(&self, table_name: &str, id: &str) -> Result<Value>;
        async fn get_table_data_as_value_some(&self, table_name: &str) -> Result<Option<Value>>;
        async fn insert_table_data_with_id_value(&self, table_name: &str, id: &str, record: Value) -> Result<()>;
        async fn replace_table_data_with_id_value(&self, table_name: &str, id: &str, record: Value) -> Result<()>;
        async fn get_count(&self, table_name: &str) -> Result<i64>;
        async fn get_sum(&self, table_name: &str, column_name: &str) -> Result<i64>;
    }
}

/// MockBuilder that wraps expressions MockBuilder and adds TableSource capabilities
#[derive(Debug)]
pub struct MockBuilder {
    expr_mock: expr_mockbuilder::MockBuilder,
    table_source: MockTableSourceImpl,
    table_methods: MockTableMethods,
}

impl Clone for MockBuilder {
    fn clone(&self) -> Self {
        Self {
            expr_mock: self.expr_mock.clone(),
            table_source: self.table_source.clone(),
            table_methods: MockTableMethods::new(),
        }
    }
}

impl MockBuilder {
    /// Create a new mock builder
    pub fn new() -> Self {
        Self {
            expr_mock: expr_mockbuilder::new(),
            table_source: MockTableSourceImpl::new(),
            table_methods: MockTableMethods::new(),
        }
    }

    /// Enable expression flattening before pattern matching
    pub fn with_flattening(mut self) -> Self {
        self.expr_mock = self.expr_mock.with_flattening();
        self
    }

    /// Add an exact pattern match for select queries
    pub fn on_exact_select(mut self, pattern: impl Into<String>, response: Value) -> Self {
        self.expr_mock = self.expr_mock.on_exact_select(pattern, response);
        self
    }

    /// Get mutable reference to table methods mock for setup
    pub fn table_mock(&mut self) -> &mut MockTableMethods {
        &mut self.table_methods
    }
}

impl Default for MockBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// Deref to expressions MockBuilder for convenience
impl Deref for MockBuilder {
    type Target = expr_mockbuilder::MockBuilder;

    fn deref(&self) -> &Self::Target {
        &self.expr_mock
    }
}

impl DataSource for MockBuilder {}

impl QuerySource<Value> for MockBuilder {
    async fn execute(&self, expr: &Expression<Value>) -> Result<Value> {
        self.expr_mock.execute(expr).await
    }

    fn defer(&self, expr: Expression<Value>) -> DeferredFn<Value>
    where
        Value: Clone + Send + Sync + 'static,
    {
        self.expr_mock.defer(expr)
    }
}

impl SelectSource<Value> for MockBuilder {
    type Select = MockSelect;

    fn select(&self) -> Self::Select {
        self.expr_mock.select()
    }

    async fn execute_select(&self, select: &Self::Select) -> Result<Vec<Value>> {
        self.expr_mock.execute_select(select).await
    }
}

#[async_trait]
impl TableSource for MockBuilder {
    type Column = MockColumn;
    type Value = Value;
    type Id = String;

    fn create_column(&self, name: &str, _table: impl TableLike) -> Self::Column {
        self.table_source.create_column(name)
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<ExpressiveEnum<Self::Value>>,
    ) -> Expression<Self::Value> {
        self.table_source.expr(template.into(), parameters)
    }

    fn search_expression(
        &self,
        _table: &impl TableLike,
        search_value: &str,
    ) -> Expression<Self::Value> {
        self.table_source.search_expression(search_value)
    }

    async fn get_table_data<E>(&self, table: &Table<Self, E>) -> DatasetResult<Vec<(String, E)>>
    where
        E: Entity,
        Self: Sized,
    {
        let values = self
            .table_methods
            .get_table_data_as_value(table.table_name())
            .await?;
        let mut results = Vec::new();

        for value in values {
            let id = value
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let record = Record::from(value);
            match E::from_record(record) {
                Ok(item) => results.push((id, item)),
                Err(_) => {
                    return Err(vantage_core::error!("Failed to convert record to entity").into());
                }
            }
        }

        Ok(results)
    }

    async fn get_table_data_some<E>(
        &self,
        table: &Table<Self, E>,
    ) -> DatasetResult<Option<(String, E)>>
    where
        E: Entity,
        Self: Sized,
    {
        if let Some(value) = self
            .table_methods
            .get_table_data_as_value_some(table.table_name())
            .await?
        {
            let id = value
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let record = Record::from(value);
            match E::from_record(record) {
                Ok(item) => Ok(Some((id, item))),
                Err(_) => Err(vantage_core::error!("Failed to convert record to entity").into()),
            }
        } else {
            Ok(None)
        }
    }

    async fn get_table_data_as_value<E>(&self, table: &Table<Self, E>) -> DatasetResult<Vec<Value>>
    where
        E: Entity,
        Self: Sized,
    {
        self.table_methods
            .get_table_data_as_value(table.table_name())
            .await
    }

    async fn get_table_data_as_value_by_id<E>(
        &self,
        table: &Table<Self, E>,
        id: &str,
    ) -> Result<Value>
    where
        E: Entity,
        Self: Sized,
    {
        self.table_methods
            .get_table_data_as_value_by_id(table.table_name(), id)
            .await
    }

    async fn get_table_data_as_value_some<E>(&self, table: &Table<Self, E>) -> Result<Option<Value>>
    where
        E: Entity,
        Self: Sized,
    {
        self.table_methods
            .get_table_data_as_value_some(table.table_name())
            .await
    }

    async fn insert_table_data<E>(
        &self,
        _table: &Table<Self, E>,
        _record: E,
    ) -> DatasetResult<Option<String>>
    where
        E: Entity + serde::Serialize,
        Self: Sized,
    {
        Err(vantage_core::error!("insert_table_data not implemented in mock").into())
    }

    async fn insert_table_data_with_id<E>(
        &self,
        _table: &Table<Self, E>,
        _id: Self::Id,
        _record: E,
    ) -> Result<()>
    where
        E: Entity + serde::Serialize,
        Self: Sized,
    {
        Err(vantage_core::error!("insert_table_data_with_id not implemented in mock").into())
    }

    async fn replace_table_data_with_id<E>(
        &self,
        _table: &Table<Self, E>,
        _id: Self::Id,
        _record: E,
    ) -> Result<()>
    where
        E: Entity + serde::Serialize,
        Self: Sized,
    {
        Err(vantage_core::error!("replace_table_data_with_id not implemented in mock").into())
    }

    async fn patch_table_data_with_id<E>(
        &self,
        _table: &Table<Self, E>,
        _id: Self::Id,
        _partial: Value,
    ) -> Result<()>
    where
        E: Entity,
        Self: Sized,
    {
        Err(vantage_core::error!("patch_table_data_with_id not implemented in mock").into())
    }

    async fn delete_table_data_with_id<E>(
        &self,
        _table: &Table<Self, E>,
        _id: Self::Id,
    ) -> Result<()>
    where
        E: Entity,
        Self: Sized,
    {
        Err(vantage_core::error!("delete_table_data_with_id not implemented in mock").into())
    }

    async fn update_table_data<E, F>(&self, _table: &Table<Self, E>, _callback: F) -> Result<()>
    where
        E: Entity,
        F: Fn(&mut E) + Send + Sync,
        Self: Sized,
    {
        Err(vantage_core::error!("update_table_data not implemented in mock").into())
    }

    async fn delete_table_data<E>(&self, _table: &Table<Self, E>) -> Result<()>
    where
        E: Entity,
        Self: Sized,
    {
        Err(vantage_core::error!("delete_table_data not implemented in mock").into())
    }

    async fn get_table_data_by_id<E>(&self, _table: &Table<Self, E>, _id: Self::Id) -> Result<E>
    where
        E: Entity,
        Self: Sized,
    {
        Err(vantage_core::error!("get_table_data_by_id not implemented in mock").into())
    }

    async fn insert_table_data_with_id_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        record: Value,
    ) -> Result<()>
    where
        E: Entity,
        Self: Sized,
    {
        self.table_methods
            .insert_table_data_with_id_value(table.table_name(), id, record)
            .await
    }

    async fn replace_table_data_with_id_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        record: Value,
    ) -> Result<()>
    where
        E: Entity,
        Self: Sized,
    {
        self.table_methods
            .replace_table_data_with_id_value(table.table_name(), id, record)
            .await
    }

    async fn update_table_data_value<E, F>(
        &self,
        _table: &Table<Self, E>,
        _callback: F,
    ) -> Result<()>
    where
        E: Entity,
        F: Fn(&mut Value) + Send + Sync,
        Self: Sized,
    {
        Err(vantage_core::error!("update_table_data_value not implemented in mock").into())
    }

    async fn get_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity,
        Self: Sized,
    {
        self.table_methods.get_count(table.table_name()).await
    }

    async fn get_sum<E>(&self, table: &Table<Self, E>, column: &Self::Column) -> Result<i64>
    where
        E: Entity,
        Self: Sized,
    {
        self.table_methods
            .get_sum(table.table_name(), column.name())
            .await
    }
}

/// Create a new mock builder
pub fn new() -> MockBuilder {
    MockBuilder::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use vantage_expressions::expr;

    #[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq, Default, Clone)]
    struct TestUser {
        id: i32,
        name: String,
    }

    #[tokio::test]
    async fn test_expression_functionality() {
        let mock = new().on_exact_select(
            "SELECT * FROM users",
            json!([
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"}
            ]),
        );

        let query = expr!("SELECT * FROM users");
        let result = mock.execute(&query).await.unwrap();
        assert_eq!(result.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_table_source_functionality() {
        let mut mock = new();

        // Setup table mock expectations
        mock.table_mock()
            .expect_get_table_data_as_value()
            .returning(|_| {
                Ok(vec![
                    json!({"id": 1, "name": "Alice"}),
                    json!({"id": 2, "name": "Bob"}),
                ])
            });

        let table = Table::<MockBuilder, TestUser>::new("users", mock);
        let data = table
            .data_source()
            .get_table_data_as_value(&table)
            .await
            .unwrap();
        assert_eq!(data.len(), 2);
    }
}
