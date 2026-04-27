use indexmap::IndexMap;
use vantage_expressions::{Expression, Expressive, traits::datasource::ExprDataSource};
use vantage_types::Entity;

use crate::{
    column::core::ColumnType, prelude::ColumnLike, table::Table, traits::table_source::TableSource,
};

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Add a column to the table (accepts any typed column, converts to `Column<AnyType>`)
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

    /// Add an ID column — sets both the column and the id_field flag.
    pub fn with_id_column(mut self, name: impl Into<String>) -> Self
    where
        T::Id: ColumnType,
    {
        let name = name.into();
        self.id_field = Some(name.clone());
        let column = self.data_source.create_column::<T::Id>(&name);
        self.add_column(column);
        self
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

    /// Get all columns as type-erased columns (`Column<AnyType>`)
    pub fn columns(&self) -> &IndexMap<String, T::Column<T::AnyType>> {
        &self.columns
    }

    /// Get a typed column by converting from stored `Column<AnyType>`
    pub fn get_column<Type>(&self, name: &str) -> Option<T::Column<Type>>
    where
        Type: ColumnType,
    {
        let any_column = self.columns.get(name)?;
        self.data_source
            .convert_any_column::<Type>(any_column.clone())
    }

    /// Get an expression for a column or computed expression by name.
    ///
    /// If `name` matches a registered expression (from `with_expression`), evaluates
    /// and returns it. Otherwise returns the column as an expression. Returns `None`
    /// if the name doesn't match either.
    pub fn get_column_expr(&self, name: &str) -> Option<vantage_expressions::Expression<T::Value>>
    where
        T::Column<T::AnyType>: vantage_expressions::Expressive<T::Value>,
    {
        if let Some(expr_fn) = self.expressions.get(name) {
            Some(expr_fn(self))
        } else {
            use vantage_expressions::Expressive;
            self.columns.get(name).map(|c| c.expr())
        }
    }
}

impl<T, E> Table<T, E>
where
    T: TableSource + ExprDataSource<T::Value>,
    E: Entity<T::Value> + 'static,
{
    /// Expression yielding all values of the named column under the
    /// table's current conditions.
    ///
    /// SQL backends materialise this as a `SELECT col FROM tbl WHERE …`
    /// subquery (embeddable directly into IN clauses); non-query
    /// backends wrap a `DeferredFn` that runs `list_table_values` and
    /// projects the column at execute time.
    ///
    /// Panics if `column_name` isn't a column on this table — the
    /// callsite is meant to be a literal column reference, so a typo
    /// is a programmer error, not a runtime failure mode worth
    /// surfacing as `Result`.
    pub fn column_values_expr(&self, column_name: &str) -> Expression<T::Value> {
        let col = self
            .get_column::<T::AnyType>(column_name)
            .unwrap_or_else(|| panic!("column {column_name:?} not found on table"));
        self.data_source
            .column_table_values_expr(self, &col)
            .expr()
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
