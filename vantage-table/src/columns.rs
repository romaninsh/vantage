use indexmap::IndexMap;
use std::ops::Index;
use vantage_expressions::{DataSource, Expression, IntoExpressive, expr};

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

/// Operations trait for columns, providing comparison and other SQL operations
pub trait ColumnOperations {
    /// Equal to comparison
    fn eq(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression;

    /// Not equal to comparison
    fn ne(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression;

    /// Greater than comparison
    fn gt(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression;

    /// Less than comparison
    fn lt(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression;

    /// Greater than or equal comparison
    fn gte(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression;

    /// Less than or equal comparison
    fn lte(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression;

    /// IN operator for lists
    fn in_list(&self, items: impl Into<IntoExpressive<Expression>>) -> Expression;

    /// IS NULL check
    fn is_null(&self) -> Expression;

    /// IS NOT NULL check
    fn is_not_null(&self) -> Expression;

    /// LIKE pattern matching
    fn like(&self, pattern: impl Into<IntoExpressive<Expression>>) -> Expression;
}

impl ColumnOperations for Column {
    fn eq(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} = {}", self, other.into())
    }

    fn ne(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} != {}", self, other.into())
    }

    fn gt(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} > {}", self, other.into())
    }

    fn lt(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} < {}", self, other.into())
    }

    fn gte(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} >= {}", self, other.into())
    }

    fn lte(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} <= {}", self, other.into())
    }

    fn in_list(&self, items: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} IN ({})", self, items.into())
    }

    fn is_null(&self) -> Expression {
        expr!("{} IS NULL", self)
    }

    fn is_not_null(&self) -> Expression {
        expr!("{} IS NOT NULL", self)
    }

    fn like(&self, pattern: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} LIKE {}", self, pattern.into())
    }
}

impl ColumnOperations for &Column {
    fn eq(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} = {}", *self, other.into())
    }

    fn ne(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} != {}", *self, other.into())
    }

    fn gt(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} > {}", *self, other.into())
    }

    fn lt(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} < {}", *self, other.into())
    }

    fn gte(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} >= {}", *self, other.into())
    }

    fn lte(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} <= {}", *self, other.into())
    }

    fn in_list(&self, items: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} IN ({})", *self, items.into())
    }

    fn is_null(&self) -> Expression {
        expr!("{} IS NULL", *self)
    }

    fn is_not_null(&self) -> Expression {
        expr!("{} IS NOT NULL", *self)
    }

    fn like(&self, pattern: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} LIKE {}", *self, pattern.into())
    }
}

impl From<String> for Column {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

impl From<&str> for Column {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl<T: DataSource<Expression>, E: Entity> Table<T, E> {
    pub fn with_column(mut self, column: impl Into<Column>) -> Self {
        self.add_column(column.into());
        self
    }

    /// Add a column to the table
    pub fn add_column(&mut self, column: Column) {
        self.columns.insert(column.name().to_string(), column);
    }

    /// Get all columns
    pub fn columns(&self) -> &IndexMap<String, Column> {
        &self.columns
    }
}

impl<T: DataSource<Expression>, E: Entity> Index<&str> for Table<T, E> {
    type Output = Column;

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

    #[test]
    fn test_column_operations_with_table() {
        let datasource = StaticDataSource::new(serde_json::json!([]));
        let table = Table::new("users", datasource)
            .with_column("age")
            .with_column("name")
            .with_column("is_vip");

        // Test column operations through table access
        let age_col = table.column("age").unwrap();
        let expr = age_col.gt(18);
        assert_eq!(expr.preview(), "age > 18");

        let name_col = table.column("name").unwrap();
        let expr = name_col.eq("John");
        assert_eq!(expr.preview(), "name = \"John\"");

        // Test building conditions - create expressions first, then move table
        let age_condition = age_col.gte(21);
        let name_condition = name_col.like("J%");

        let table_with_conditions = table
            .with_condition(age_condition)
            .with_condition(name_condition);

        assert_eq!(table_with_conditions.conditions.len(), 2);
    }

    #[test]
    fn test_column_operations() {
        let col = Column::new("age");

        // Test eq
        let expr = col.eq(25);
        assert_eq!(expr.preview(), "age = 25");

        // Test gt
        let expr = col.gt(18);
        assert_eq!(expr.preview(), "age > 18");

        // Test lt
        let expr = col.lt(65);
        assert_eq!(expr.preview(), "age < 65");

        // Test in_list
        let expr = col.in_list(vec![1, 2, 3]);
        assert_eq!(expr.preview(), "age IN ([1,2,3])");

        // Test is_null
        let expr = col.is_null();
        assert_eq!(expr.preview(), "age IS NULL");

        // Test like
        let expr = col.like("John%");
        assert_eq!(expr.preview(), "age LIKE \"John%\"");
    }

    #[test]
    fn test_column_operations_reference() {
        let col = Column::new("name");

        // Test with reference
        let expr = (&col).eq("John");
        assert_eq!(expr.preview(), "name = \"John\"");

        let expr = (&col).ne("Jane");
        assert_eq!(expr.preview(), "name != \"Jane\"");
    }
}
