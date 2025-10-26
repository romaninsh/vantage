use indexmap::IndexMap;
use std::collections::HashSet;
use std::ops::Index;
use vantage_expressions::{Expression, IntoExpressive, expr};

use crate::ColumnLike;

use super::{Entity, Table, TableSource};

/// Column flags that define behavior and constraints
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ColumnFlag {
    /// Mandatory will require read/write operations to always have value for this field, it cannot be missing
    Mandatory,
}

/// Represents a table column with optional alias and flags
#[derive(Debug, Clone)]
pub struct Column {
    name: String,
    alias: Option<String>,
    flags: HashSet<ColumnFlag>,
}

impl Column {
    /// Create a new column with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: None,
            flags: HashSet::new(),
        }
    }

    /// Set an alias for this column
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }

    /// Get the column name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the column alias if set
    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    /// Add flags to this column
    pub fn with_flags(mut self, flags: &[ColumnFlag]) -> Self {
        self.flags.extend(flags.iter().cloned());
        self
    }

    /// Get the column flags
    pub fn flags(&self) -> &HashSet<ColumnFlag> {
        &self.flags
    }

    /// Create an expression from this column name
    pub fn expr(&self) -> Expression {
        expr!(self.name.clone())
    }
}

impl ColumnLike for Column {
    fn name(&self) -> &str {
        &self.name
    }

    fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    fn expr(&self) -> Expression {
        expr!(self.name.clone())
    }

    fn flags(&self) -> HashSet<ColumnFlag> {
        self.flags.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn get_type(&self) -> &'static str {
        "any"
    }
}

impl From<&str> for Column {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl<T: TableSource, E: Entity> Table<T, E>
where
    T::Column: ColumnLike,
{
    pub fn with_column(mut self, column: impl Into<T::Column>) -> Self {
        let column = column.into();
        self.columns.insert(column.name().to_string(), column);
        self
    }

    /// Add a column to the table
    pub fn add_column(&mut self, column: impl Into<T::Column>) {
        let column = column.into();
        self.columns.insert(column.name().to_string(), column);
    }

    /// Add an ID column to the table (typically String type for most databases)
    /// This is a convenience method for defining the primary key column
    pub fn with_id_column(self, name: impl Into<String>) -> Self
    where
        T::Column: From<Column>,
    {
        self.with_column(Column::new(name.into()))
    }

    /// Get all columns
    pub fn columns(&self) -> &IndexMap<String, T::Column> {
        &self.columns
    }

    /// Get a reference to a column for operations
    pub fn column(&self, name: &str) -> Option<&T::Column> {
        self.columns.get(name)
    }
}

impl<T: TableSource, E: Entity> Index<&str> for Table<T, E> {
    type Output = T::Column;

    fn index(&self, index: &str) -> &Self::Output {
        &self.columns[index]
    }
}

impl From<Column> for IntoExpressive<Expression> {
    fn from(val: Column) -> Self {
        IntoExpressive::nested(val.expr())
    }
}

impl From<&Column> for IntoExpressive<Expression> {
    fn from(val: &Column) -> Self {
        IntoExpressive::nested(val.expr())
    }
}

impl From<Column> for String {
    fn from(val: Column) -> Self {
        val.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::MockTableSource;
    use crate::with_ordering::OrderByExt;
    use vantage_expressions::expr;

    #[test]
    fn test_column_in_expression() {
        let datasource = MockTableSource::new();
        let table = Table::new("users", datasource)
            .with_column("is_vip")
            .with_column("name");

        let expr = expr!("{} = true", &table["is_vip"]);
        assert_eq!(expr.preview(), "is_vip = true");
    }

    #[test]
    fn test_column_with_flags() {
        let column = Column::new("email").with_flags(&[ColumnFlag::Mandatory]);
        assert!(column.flags().contains(&ColumnFlag::Mandatory));
        assert_eq!(column.flags().len(), 1);
    }

    #[test]
    fn test_column_no_flags() {
        let column = Column::new("optional_field");
        assert!(column.flags().is_empty());
    }

    #[test]
    fn test_column_ordering() {
        use crate::with_ordering::SortDirection;

        let datasource = MockTableSource::new();
        let table = Table::new("users", datasource)
            .with_column("name")
            .with_column("age");

        let mut table = table;

        // Test column.ascending()
        table.add_order(table["name"].ascending());
        assert_eq!(table.orders().count(), 1);

        // Test column.descending()
        table.add_order(table["age"].descending());
        assert_eq!(table.orders().count(), 2);

        // Verify directions
        let orders: Vec<_> = table.orders().collect();
        assert!(matches!(orders[0].1, SortDirection::Ascending));
        assert!(matches!(orders[1].1, SortDirection::Descending));
    }
}
