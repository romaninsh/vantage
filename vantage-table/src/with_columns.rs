use indexmap::IndexMap;
use std::ops::Index;
use vantage_expressions::{
    DataSource, Expression, IntoExpressive, expr, protocol::datasource::ColumnLike,
};

use super::{Entity, Table};

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

impl<T: DataSource<Expression>, E: Entity> Table<T, E>
where
    T::Column: ColumnLike,
{
    pub fn with_column(mut self, column: impl Into<T::Column>) -> Self {
        self.add_column(column);
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

impl<T: DataSource<Expression>, E: Entity> Index<&str> for Table<T, E> {
    type Output = T::Column;

    fn index(&self, index: &str) -> &Self::Output {
        &self.columns[index]
    }
}

impl Into<IntoExpressive<Expression>> for Column {
    fn into(self) -> IntoExpressive<Expression> {
        IntoExpressive::nested(self.expr())
    }
}

impl Into<IntoExpressive<Expression>> for &Column {
    fn into(self) -> IntoExpressive<Expression> {
        IntoExpressive::nested(self.expr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_expressions::{expr, mocks::StaticDataSource};

    #[test]
    fn test_column_in_expression() {
        let datasource = StaticDataSource::new(serde_json::json!([]));
        let table = Table::new("users", datasource)
            .with_column("is_vip")
            .with_column("name");

        let expr = expr!("{} = true", &table["is_vip"]);
        assert_eq!(expr.preview(), "is_vip = true");
    }
}
