use indexmap::IndexMap;
use std::ops::Index;
use vantage_expressions::{Expression, IntoExpressive, expr};

use crate::ColumnLike;

use super::{Entity, Table, TableSource};

/// Represents a table column with optional alias
#[derive(Debug, Clone)]
pub struct Column {
    name: String,
    alias: Option<String>,
}

impl Column {
    /// Create a new column with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: None,
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
    pub fn with_column(mut self, column: impl Into<String>) -> Self {
        let column_name = column.into();
        let column = self.data_source().create_column(&column_name, &self);
        self.columns.insert(column.name().to_string(), column);
        self
    }

    /// Add a column to the table
    pub fn add_column(&mut self, column: impl Into<T::Column>) {
        let column = column.into();
        self.columns.insert(column.name().to_string(), column);
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
}
