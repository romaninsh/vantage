//! Type-erased table wrapper with downcasting support
//!
//! `AnyTable` provides a way to store tables of different types uniformly
//! while preserving the ability to recover the concrete type through downcasting.

use std::any::TypeId;

use async_trait::async_trait;
use vantage_core::{Result, error};
use vantage_dataset::dataset::{ReadableValueSet, WritableValueSet};

use crate::{Entity, Table, TableLike, TableSource};

/// Type-erased table that can be downcast to concrete `Table<T, E>`
pub struct AnyTable {
    inner: Box<dyn TableLike>,
    datasource_type_id: TypeId,
    entity_type_id: TypeId,
    datasource_name: &'static str,
    entity_name: &'static str,
}

impl AnyTable {
    /// Create a new AnyTable from a concrete table
    pub fn new<T: TableSource + 'static, E: Entity + 'static>(table: Table<T, E>) -> Self {
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

// Implement ReadableValueSet by delegating to inner TableLike
#[async_trait]
impl ReadableValueSet for AnyTable {
    async fn get_values(&self) -> Result<Vec<serde_json::Value>> {
        self.inner.get_values().await
    }

    async fn get_id_value(&self, id: &str) -> Result<serde_json::Value> {
        self.inner.get_id_value(id).await
    }

    async fn get_some_value(&self) -> Result<Option<serde_json::Value>> {
        self.inner.get_some_value().await
    }
}

// Implement WritableValueSet by delegating to inner TableLike
#[async_trait]
impl WritableValueSet for AnyTable {
    async fn insert_id_value(&self, id: &str, record: serde_json::Value) -> Result<()> {
        self.inner.insert_id_value(id, record).await
    }

    async fn replace_id_value(&self, id: &str, record: serde_json::Value) -> Result<()> {
        self.inner.replace_id_value(id, record).await
    }

    async fn patch_id(&self, id: &str, partial: serde_json::Value) -> Result<()> {
        self.inner.patch_id(id, partial).await
    }

    async fn delete_id(&self, id: &str) -> Result<()> {
        self.inner.delete_id(id).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.inner.delete_all().await
    }
}

// Implement TableLike by delegating to inner
#[async_trait]
impl TableLike for AnyTable {
    fn columns(
        &self,
    ) -> std::sync::Arc<indexmap::IndexMap<String, std::sync::Arc<dyn crate::ColumnLike>>> {
        self.inner.columns()
    }

    fn get_column(&self, name: &str) -> Option<std::sync::Arc<dyn crate::ColumnLike>> {
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
    ) -> Result<crate::ConditionHandle> {
        self.inner.temp_add_condition(condition)
    }

    fn temp_remove_condition(&mut self, handle: crate::ConditionHandle) -> Result<()> {
        self.inner.temp_remove_condition(handle)
    }

    fn search_expression(&self, search_value: &str) -> Result<vantage_expressions::AnyExpression> {
        self.inner.search_expression(search_value)
    }

    fn clone_box(&self) -> Box<dyn TableLike> {
        Box::new(self.clone())
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }

    fn as_any_ref(&self) -> &dyn std::any::Any {
        self
    }

    fn set_pagination(&mut self, pagination: Option<crate::Pagination>) {
        self.inner.set_pagination(pagination)
    }

    fn get_pagination(&self) -> Option<&crate::Pagination> {
        self.inner.get_pagination()
    }

    async fn get_count(&self) -> vantage_core::Result<i64> {
        self.inner.get_count().await
    }

    async fn get_sum(&self, column: &dyn crate::ColumnLike) -> vantage_core::Result<i64> {
        self.inner.get_sum(column).await
    }
}

impl AnyTable {
    /// Configure pagination using a callback
    pub fn with_pagination<F>(&mut self, func: F)
    where
        F: FnOnce(&mut crate::Pagination),
    {
        let mut pagination = self.inner.get_pagination().copied().unwrap_or_default();
        func(&mut pagination);
        self.inner.set_pagination(Some(pagination));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EmptyEntity;
    use crate::mocks::MockTableSource;

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
    struct TestEntity {
        id: i32,
        name: String,
    }

    #[test]
    fn test_anytable_creation_and_downcast() {
        let ds = MockTableSource::new();
        let table = Table::new("test", ds).into_entity::<TestEntity>();
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
        let table = Table::new("test", ds).into_entity::<TestEntity>();
        let any = AnyTable::new(table);

        // Try to downcast to wrong entity type
        let result = any.downcast::<MockTableSource, EmptyEntity>();
        assert!(result.is_err());
    }

    #[test]
    fn test_anytable_is_type() {
        let ds = MockTableSource::new();
        let table = Table::new("test", ds).into_entity::<TestEntity>();
        let any = AnyTable::new(table);

        assert!(any.is_type::<MockTableSource, TestEntity>());
        assert!(!any.is_type::<MockTableSource, EmptyEntity>());
    }

    #[test]
    fn test_anytable_debug() {
        let ds = MockTableSource::new();
        let table = Table::new("test", ds);
        let any = AnyTable::new(table);

        let debug_str = format!("{:?}", any);
        assert!(debug_str.contains("AnyTable"));
        assert!(debug_str.contains("datasource"));
        assert!(debug_str.contains("entity"));
    }
}
