use indexmap::IndexMap;
use vantage_types::Entity;

use crate::{
    column::column::ColumnType, prelude::ColumnLike, table::Table,
    traits::table_source::TableSource,
};

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Add a column to the table (accepts any typed column, converts to Column<AnyType>)
    pub fn add_column<NewColumnType>(&mut self, column: T::Column<NewColumnType>)
    where
        NewColumnType: ColumnType,
    {
        let name = column.name().to_string();

        if self.columns.contains_key(&name) {
            panic!("Duplicate column: {}", name);
        }

        // Convert typed column to Column<AnyType> for storage
        let any_column = self.data_source.to_any_column(column);
        self.columns.insert(name, any_column);
    }

    /// Add a column using builder pattern
    pub fn with_column<NewColumnType>(mut self, column: T::Column<NewColumnType>) -> Self
    where
        NewColumnType: ColumnType,
    {
        self.add_column(column);
        self
    }

    /// Add a typed column to the table (mutable)
    pub fn add_column_of<NewColumnType>(&mut self, name: impl Into<String>)
    where
        NewColumnType: ColumnType,
    {
        let column = self
            .data_source
            .create_column::<NewColumnType>(&name.into());
        self.add_column(column);
    }

    /// Add a typed column to the table (builder pattern)
    pub fn with_column_of<NewColumnType>(self, name: impl Into<String>) -> Self
    where
        NewColumnType: ColumnType,
    {
        let column = self
            .data_source
            .create_column::<NewColumnType>(&name.into());
        self.with_column(column)
    }

    /// Get all columns as type-erased columns (Column<AnyType>)
    pub fn columns(&self) -> &IndexMap<String, T::Column<T::AnyType>> {
        &self.columns
    }

    /// Get a typed column by converting from stored Column<AnyType>
    pub fn get_column<Type>(&self, name: &str) -> Option<T::Column<Type>>
    where
        Type: crate::column::column::ColumnType,
    {
        let any_column = self.columns.get(name)?;
        self.data_source.from_any_column::<Type>(any_column.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::mock_column::MockColumn;
    use crate::prelude::MockTableSource;
    use serde_json::Value;
    use vantage_types::EmptyEntity;

    #[test]
    fn test_add_column() {
        let ds = MockTableSource::new();
        let mut table = Table::<MockTableSource, EmptyEntity>::new("test", ds);

        table.add_column(MockColumn::<String>::new("name"));

        assert!(table.columns().contains_key("name"));
        assert_eq!(table.columns().len(), 1);
    }

    #[test]
    fn test_with_column() {
        let ds = MockTableSource::new();
        let table = Table::<MockTableSource, EmptyEntity>::new("test", ds)
            .with_column(MockColumn::<Value>::new("name"))
            .with_column(MockColumn::<i32>::new("email"));

        assert!(table.columns().contains_key("name"));
        assert!(table.columns().contains_key("email"));
        assert_eq!(table.columns().len(), 2);
    }

    #[test]
    #[should_panic(expected = "Duplicate column")]
    fn test_duplicate_column_panics() {
        let ds = MockTableSource::new();
        let mut table = Table::<MockTableSource, EmptyEntity>::new("test", ds);

        table.add_column(MockColumn::<String>::new("name"));
        table.add_column(MockColumn::<String>::new("name")); // Should panic
    }

    #[test]
    fn test_with_column_of() {
        let ds = MockTableSource::new();
        let table = Table::<MockTableSource, EmptyEntity>::new("test", ds)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("age")
            .with_column_of::<bool>("active");

        assert!(table.columns().contains_key("name"));
        assert!(table.columns().contains_key("age"));
        assert!(table.columns().contains_key("active"));
        assert_eq!(table.columns().len(), 3);
    }

    #[test]
    fn test_add_column_of() {
        let ds = MockTableSource::new();
        let mut table = Table::<MockTableSource, EmptyEntity>::new("test", ds);

        table.add_column_of::<String>("email");
        table.add_column_of::<i64>("balance");

        assert!(table.columns().contains_key("email"));
        assert!(table.columns().contains_key("balance"));
        assert_eq!(table.columns().len(), 2);
    }

    #[test]
    fn test_columns_access() {
        let ds = MockTableSource::new();
        let table = Table::<MockTableSource, EmptyEntity>::new("test", ds)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("age");

        let columns = table.columns();
        assert!(columns.contains_key("name"));
        assert!(columns.contains_key("age"));
        assert_eq!(columns.len(), 2);

        let name_column = table.columns().get("name");
        assert!(name_column.is_some());
        assert_eq!(name_column.unwrap().name(), "name");

        let missing_column = table.columns().get("missing");
        assert!(missing_column.is_none());
    }
}
