use crate::column::column::ColumnType;
use crate::column::flags::ColumnFlag;
use crate::traits::column_like::ColumnLike;
use std::any::{Any, TypeId};
use std::collections::HashSet;
use vantage_core::*;

/// Helper trait for cloning type-erased columns
trait CloneColumn: Send + Sync {
    fn clone_column(&self) -> Box<dyn CloneColumn>;
    fn as_any(&self) -> &dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send + Sync>;
}

impl<T, C> CloneColumn for C
where
    C: ColumnLike<T> + Clone + Send + Sync + 'static,
    T: ColumnType,
{
    fn clone_column(&self) -> Box<dyn CloneColumn> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any + Send + Sync> {
        self as Box<dyn Any + Send + Sync>
    }
}

/// Type-erased column that can hold any column type
pub struct AnyColumn {
    name: String,
    alias: Option<String>,
    flags: HashSet<ColumnFlag>,
    type_id: TypeId,
    type_name: &'static str,
    inner: Box<dyn CloneColumn>,
}

impl AnyColumn {
    /// Create AnyColumn from any column type with true type erasure
    pub fn new<T, C>(column: C) -> Self
    where
        C: ColumnLike<T> + Clone + Send + Sync + 'static,
        T: ColumnType,
    {
        // Extract basic column information before erasure
        let name = column.name().to_string();
        let alias = column.alias().map(|s| s.to_string());
        let flags = column.flags();
        let type_id = TypeId::of::<C>();
        let type_name = std::any::type_name::<C>();

        Self {
            name,
            alias,
            flags,
            type_id,
            type_name,
            inner: Box::new(column),
        }
    }

    /// Attempt to downcast to a concrete column type
    pub fn downcast<C: 'static>(self) -> Result<C> {
        self.inner
            .into_any()
            .downcast::<C>()
            .map(|boxed| *boxed)
            .map_err(|_| error!("Failed to downcast column"))
    }

    /// Get type information
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Get type name for debugging
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }

    /// Check if this column is of a specific type
    pub fn is_type<C: 'static>(&self) -> bool {
        self.type_id == TypeId::of::<C>()
    }
}

impl AnyColumn {
    /// Get the column name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the column alias
    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    /// Get the column flags
    pub fn flags(&self) -> &HashSet<ColumnFlag> {
        &self.flags
    }
}

impl Clone for AnyColumn {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            alias: self.alias.clone(),
            flags: self.flags.clone(),
            type_id: self.type_id,
            type_name: self.type_name,
            inner: self.inner.clone_column(),
        }
    }
}

impl std::fmt::Debug for AnyColumn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnyColumn")
            .field("name", &self.name)
            .field("alias", &self.alias)
            .field("type_name", &self.type_name)
            .field("flags_count", &self.flags.len())
            .finish()
    }
}

// Note: AnyColumn is deprecated in favor of TableSource-specific AnyColumn types
// Each TableSource now defines its own AnyColumn type for better type safety

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::{column::MockColumn, type_column::TypeColumn};
    use serde_json::Value;

    #[test]
    fn test_any_column_with_mock_column() {
        let mock_col = MockColumn::<Value>::new("test_column");
        let any_col = AnyColumn::new(mock_col);

        assert_eq!(any_col.name(), "test_column");
        assert_eq!(any_col.alias(), None);
        assert!(any_col.is_type::<MockColumn<Value>>());
        assert!(!any_col.is_type::<TypeColumn<String>>());
    }

    #[test]
    fn test_any_column_with_type_column_string() {
        let type_col = TypeColumn::string("name_field");
        let any_col = AnyColumn::new(type_col);

        assert_eq!(any_col.name(), "name_field");
        assert_eq!(any_col.alias(), None);
        assert!(any_col.is_type::<TypeColumn<String>>());
        assert!(!any_col.is_type::<TypeColumn<i64>>());
        assert_eq!(
            any_col.type_name(),
            "vantage_table::mocks::type_column::TypeColumn<alloc::string::String>"
        );
    }

    #[test]
    fn test_any_column_with_type_column_integer() {
        let type_col = TypeColumn::<i64>::new("age_field");
        let any_col = AnyColumn::new(type_col);

        assert_eq!(any_col.name(), "age_field");
        assert!(any_col.is_type::<TypeColumn<i64>>());
        assert!(!any_col.is_type::<TypeColumn<String>>());
    }

    #[test]
    fn test_any_column_with_type_column_boolean() {
        let type_col = TypeColumn::<bool>::new("active_field");
        let any_col = AnyColumn::new(type_col);

        assert_eq!(any_col.name(), "active_field");
        assert!(any_col.is_type::<TypeColumn<bool>>());
        assert!(!any_col.is_type::<TypeColumn<String>>());
    }

    #[test]
    fn test_any_column_downcast_success() {
        let original = TypeColumn::string("test_field");
        let any_col = AnyColumn::new(original);

        // Downcast back to original type
        let recovered: TypeColumn<String> = any_col.downcast().unwrap();
        assert_eq!(recovered.name(), "test_field");
    }

    #[test]
    fn test_any_column_downcast_failure() {
        let type_col = TypeColumn::string("test_field");
        let any_col = AnyColumn::new(type_col);

        // Try to downcast to wrong type
        let result: Result<TypeColumn<i64>> = any_col.downcast();
        assert!(result.is_err());
    }

    #[test]
    fn test_any_column_type_information() {
        let type_col = TypeColumn::<bool>::new("flag");
        let type_id_before = std::any::TypeId::of::<TypeColumn<bool>>();

        let any_col = AnyColumn::new(type_col);

        assert_eq!(any_col.type_id(), type_id_before);
        assert!(any_col.type_name().contains("TypeColumn"));
        assert!(any_col.type_name().contains("bool"));
    }

    #[test]
    fn test_any_column_mixed_storage() {
        // Simulate storing different column types together
        let mut columns: Vec<AnyColumn> = Vec::new();

        columns.push(AnyColumn::new(MockColumn::<Value>::new("mock_col")));
        columns.push(AnyColumn::new(TypeColumn::string("string_col")));
        columns.push(AnyColumn::new(TypeColumn::<i64>::new("int_col")));
        columns.push(AnyColumn::new(TypeColumn::<bool>::new("bool_col")));

        assert_eq!(columns.len(), 4);
        assert_eq!(columns[0].name(), "mock_col");
        assert_eq!(columns[1].name(), "string_col");
        assert_eq!(columns[2].name(), "int_col");
        assert_eq!(columns[3].name(), "bool_col");

        // Verify type information is preserved
        assert!(columns[0].is_type::<MockColumn<Value>>());
        assert!(columns[1].is_type::<TypeColumn<String>>());
        assert!(columns[2].is_type::<TypeColumn<i64>>());
        assert!(columns[3].is_type::<TypeColumn<bool>>());
    }
}
