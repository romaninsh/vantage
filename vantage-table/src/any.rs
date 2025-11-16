//! Type-erased table wrapper with downcasting support
//!
//! `AnyTable` provides a way to store tables of different types uniformly
//! while preserving the ability to recover the concrete type through downcasting.

use std::any::TypeId;

use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::{Entity, Result, error};
use vantage_dataset::traits::{ReadableValueSet, ValueSet, WritableValueSet};

use crate::{
    conditions::ConditionHandle,
    pagination::Pagination,
    table::Table,
    traits::{column_like::ColumnLike, table_like::TableLike, table_source::TableSource},
};

/// Type-erased table that can be downcast to concrete `Table<T, E>`
pub struct AnyTable {
    inner: Box<dyn TableLike<Value = serde_json::Value, Id = String>>,
    datasource_type_id: TypeId,
    entity_type_id: TypeId,
    datasource_name: &'static str,
    entity_name: &'static str,
}

impl AnyTable {
    /// Create a new AnyTable from a concrete table
    /// Only works with tables that use serde_json::Value as their Value type
    pub fn new<T: TableSource<Value = serde_json::Value> + 'static, E: Entity + 'static>(
        table: Table<T, E>,
    ) -> Self {
        Self {
            inner: Box::new(table),
            datasource_type_id: TypeId::of::<T>(),
            entity_type_id: TypeId::of::<E>(),
            datasource_name: std::any::type_name::<T>(),
            entity_name: std::any::type_name::<E>(),
        }
    }

    /// Attempt to downcast to a concrete `Table<T, E>`
    ///
    /// Returns `Err(self)` if the type doesn't match, allowing recovery
    pub fn downcast<T: TableSource + 'static, E: Entity + 'static>(self) -> Result<Table<T, E>> {
        // Check TypeIds for better error messages
        if self.datasource_type_id != TypeId::of::<T>() {
            let expected = std::any::type_name::<T>();
            return Err(error!(
                "DataSource type mismatch",
                expected = expected,
                actual = self.datasource_name
            ));
        }
        if self.entity_type_id != TypeId::of::<E>() {
            let expected = std::any::type_name::<E>();
            return Err(error!(
                "Entity type mismatch",
                expected = expected,
                actual = self.entity_name
            ));
        }

        // Perform the actual downcast
        self.inner
            .into_any()
            .downcast::<Table<T, E>>()
            .map(|boxed| *boxed)
            .map_err(|_| error!("Failed to downcast table"))
    }

    /// Get the datasource type name for debugging
    pub fn datasource_name(&self) -> &str {
        self.datasource_name
    }

    /// Get the entity type name for debugging
    pub fn entity_name(&self) -> &str {
        self.entity_name
    }

    /// Get the datasource TypeId
    pub fn datasource_type_id(&self) -> TypeId {
        self.datasource_type_id
    }

    /// Get the entity TypeId
    pub fn entity_type_id(&self) -> TypeId {
        self.entity_type_id
    }

    /// Check if this table matches the given types
    pub fn is_type<T: TableSource + 'static, E: Entity + 'static>(&self) -> bool {
        self.datasource_type_id == TypeId::of::<T>() && self.entity_type_id == TypeId::of::<E>()
    }
}

impl Clone for AnyTable {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone_box(),
            datasource_type_id: self.datasource_type_id,
            entity_type_id: self.entity_type_id,
            datasource_name: self.datasource_name,
            entity_name: self.entity_name,
        }
    }
}

impl std::fmt::Debug for AnyTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnyTable")
            .field("datasource", &self.datasource_name)
            .field("entity", &self.entity_name)
            .finish()
    }
}

// Implement ValueSet first
impl ValueSet for AnyTable {
    type Id = String;
    type Value = serde_json::Value;
}

// Implement ReadableValueSet by delegating to inner TableLike
#[async_trait]
impl ReadableValueSet for AnyTable {
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Self::Value>> {
        self.inner.list_values().await
    }

    async fn get_value(&self, id: &Self::Id) -> Result<Self::Value> {
        self.inner.get_value(id).await
    }

    async fn get_some_value(&self) -> Result<Option<(Self::Id, Self::Value)>> {
        self.inner.get_some_value().await
    }
}

// Implement WritableValueSet by delegating to inner TableLike
#[async_trait]
impl WritableValueSet for AnyTable {
    async fn insert_value(&self, id: &Self::Id, record: Self::Value) -> Result<()> {
        self.inner.insert_value(id, record).await
    }

    async fn replace_value(&self, id: &Self::Id, record: Self::Value) -> Result<()> {
        self.inner.replace_value(id, record).await
    }

    async fn patch_value(&self, id: &Self::Id, partial: Self::Value) -> Result<()> {
        self.inner.patch_value(id, partial).await
    }

    async fn delete(&self, id: &Self::Id) -> Result<()> {
        self.inner.delete(id).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.inner.delete_all().await
    }
}

// Implement TableLike by delegating to inner
#[async_trait]
impl TableLike for AnyTable {
    fn columns(&self) -> std::sync::Arc<IndexMap<String, std::sync::Arc<dyn ColumnLike>>> {
        self.inner.columns()
    }

    fn get_column(&self, name: &str) -> Option<std::sync::Arc<dyn ColumnLike>> {
        self.inner.get_column(name)
    }

    fn table_name(&self) -> &str {
        self.inner.table_name()
    }

    fn table_alias(&self) -> &str {
        self.inner.table_alias()
    }

    fn add_condition(&mut self, condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        self.inner.add_condition(condition)
    }

    fn temp_add_condition(
        &mut self,
        condition: vantage_expressions::AnyExpression,
    ) -> Result<ConditionHandle> {
        self.inner.temp_add_condition(condition)
    }

    fn temp_remove_condition(&mut self, handle: ConditionHandle) -> Result<()> {
        self.inner.temp_remove_condition(handle)
    }

    fn search_expression(&self, search_value: &str) -> Result<vantage_expressions::AnyExpression> {
        self.inner.search_expression(search_value)
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
        self.inner.set_pagination(pagination)
    }

    fn get_pagination(&self) -> Option<&Pagination> {
        self.inner.get_pagination()
    }

    async fn get_count(&self) -> vantage_core::Result<i64> {
        self.inner.get_count().await
    }

    async fn get_sum(&self, column: &dyn ColumnLike) -> vantage_core::Result<i64> {
        self.inner.get_sum(column).await
    }

    fn title_field(&self) -> Option<std::sync::Arc<dyn ColumnLike>> {
        self.inner.title_field()
    }

    fn id_field(&self) -> Option<std::sync::Arc<dyn ColumnLike>> {
        self.inner.id_field()
    }
}

impl AnyTable {
    /// Configure pagination using a callback
    pub fn with_pagination<F>(&mut self, func: F)
    where
        F: FnOnce(&mut Pagination),
    {
        let mut pagination = self.inner.get_pagination().copied().unwrap_or_default();
        func(&mut pagination);
        self.inner.set_pagination(Some(pagination));
    }
}

#[cfg(test)]
mod tests {
    use crate::mocks::tablesource::MockTableSource;

    use super::*;

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
    struct TestEntity {
        id: i32,
        name: String,
    }

    #[test]
    fn test_anytable_creation_and_downcast() {
        let ds = MockTableSource::new();
        let table = Table::<MockTableSource, TestEntity>::new("test", ds);
        let any = AnyTable::new(table.clone());

        assert_eq!(
            any.datasource_name(),
            std::any::type_name::<MockTableSource>()
        );
        assert_eq!(any.entity_name(), std::any::type_name::<TestEntity>());

        // Successful downcast
        let recovered = any.downcast::<MockTableSource, TestEntity>().unwrap();
        assert_eq!(recovered.table_name(), "test");
    }

    #[test]
    fn test_anytable_downcast_wrong_entity() {
        let ds = MockTableSource::new();
        let table = Table::<MockTableSource, TestEntity>::new("test", ds);
        let any = AnyTable::new(table);

        // Try to downcast to wrong entity type - use a different entity
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
        struct DifferentEntity;

        // DifferentEntity automatically gets Entity via blanket impl

        let result = any.downcast::<MockTableSource, DifferentEntity>();
        assert!(result.is_err());
    }

    #[test]
    fn test_anytable_is_type() {
        let ds = MockTableSource::new();
        let table = Table::<MockTableSource, TestEntity>::new("test", ds);
        let any = AnyTable::new(table);

        assert!(any.is_type::<MockTableSource, TestEntity>());
        // Test with different entity type
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
        struct OtherEntity;

        assert!(!any.is_type::<MockTableSource, OtherEntity>());
    }

    #[test]
    fn test_anytable_debug() {
        let ds = MockTableSource::new();
        let table = Table::<MockTableSource, TestEntity>::new("test", ds);
        let any = AnyTable::new(table);

        let debug_str = format!("{:?}", any);
        assert!(debug_str.contains("AnyTable"));
        assert!(debug_str.contains("datasource"));
        assert!(debug_str.contains("entity"));
    }
}
