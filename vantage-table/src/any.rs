//! Type-erased table wrapper with downcasting support
//!
//! `AnyTable` provides a way to store tables of different types uniformly
//! while preserving the ability to recover the concrete type through downcasting.

use std::any::TypeId;

use vantage_core::{Result, error};

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

impl std::fmt::Debug for AnyTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnyTable")
            .field("datasource", &self.datasource_name)
            .field("entity", &self.entity_name)
            .finish()
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
