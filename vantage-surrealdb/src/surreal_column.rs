//! # SurrealDB Column
//!
//! A SurrealDB-specific column implementation that uses Identifier for proper escaping.

use crate::identifier::Identifier;
use crate::operation::Expressive;
use std::collections::HashSet;
use vantage_expressions::{Expression, IntoExpressive, expr};
use vantage_table::{ColumnFlag, ColumnLike};

/// SurrealDB-specific column that renders as an Identifier
#[derive(Debug, Clone)]
pub struct SurrealColumn {
    name: String,
    alias: Option<String>,
    flags: HashSet<ColumnFlag>,
}

impl SurrealColumn {
    /// Create a new SurrealDB column with the given name
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

    /// Add flags to this column
    pub fn with_flags(mut self, flags: &[ColumnFlag]) -> Self {
        self.flags.extend(flags.iter().cloned());
        self
    }

    /// Get the underlying identifier
    pub fn identifier(&self) -> Identifier {
        Identifier::new(&self.name)
    }
}

impl ColumnLike for SurrealColumn {
    fn name(&self) -> &str {
        &self.name
    }

    fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    fn expr(&self) -> Expression {
        self.identifier().expr()
    }

    fn flags(&self) -> HashSet<ColumnFlag> {
        self.flags.clone()
    }
}

impl From<&str> for SurrealColumn {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for SurrealColumn {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

impl From<vantage_table::Column> for SurrealColumn {
    fn from(column: vantage_table::Column) -> Self {
        let mut surreal_column = Self::new(column.name());
        if let Some(alias) = column.alias() {
            surreal_column = surreal_column.with_alias(alias);
        }
        surreal_column =
            surreal_column.with_flags(&column.flags().iter().cloned().collect::<Vec<_>>());
        surreal_column
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_surreal_column_basic() {
        let col = SurrealColumn::new("user_name");
        assert_eq!(col.name(), "user_name");
        assert_eq!(col.alias(), None);
    }

    #[test]
    fn test_surreal_column_with_alias() {
        let col = SurrealColumn::new("user_name").with_alias("name");
        assert_eq!(col.name(), "user_name");
        assert_eq!(col.alias(), Some("name"));
    }

    #[test]
    fn test_surreal_column_expr() {
        let col = SurrealColumn::new("user_name");
        let expr = col.expr();
        assert_eq!(expr.preview(), "user_name");
    }

    #[test]
    fn test_surreal_column_reserved_keyword() {
        let col = SurrealColumn::new("SELECT");
        let expr = col.expr();
        assert_eq!(expr.preview(), "⟨SELECT⟩");
    }

    #[test]
    fn test_from_str() {
        let col: SurrealColumn = "email".into();
        assert_eq!(col.name(), "email");
    }

    #[test]
    fn test_from_vantage_column() {
        let vantage_col = vantage_table::Column::new("test").with_alias("test_alias");
        let surreal_col: SurrealColumn = vantage_col.into();
        assert_eq!(surreal_col.name(), "test");
        assert_eq!(surreal_col.alias(), Some("test_alias"));
    }

    #[test]
    fn test_surreal_column_with_flags() {
        let col = SurrealColumn::new("email").with_flags(&[ColumnFlag::Mandatory]);
        assert!(col.flags().contains(&ColumnFlag::Mandatory));
        assert_eq!(col.flags().len(), 1);
    }

    #[test]
    fn test_surreal_column_no_flags() {
        let col = SurrealColumn::new("optional_field");
        assert!(col.flags().is_empty());
    }

    #[test]
    fn test_from_vantage_column_with_flags() {
        let vantage_col = vantage_table::Column::new("test")
            .with_flags(&[ColumnFlag::Mandatory])
            .with_alias("test_alias");
        let surreal_col: SurrealColumn = vantage_col.into();
        assert_eq!(surreal_col.name(), "test");
        assert_eq!(surreal_col.alias(), Some("test_alias"));
        assert!(surreal_col.flags().contains(&ColumnFlag::Mandatory));
    }
}

/// Operations trait for SurrealDB columns, providing comparison and other SQL operations
pub trait SurrealColumnOperations {
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

impl SurrealColumnOperations for SurrealColumn {
    fn eq(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} = {}", self.identifier(), other.into())
    }

    fn ne(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} != {}", self.identifier(), other.into())
    }

    fn gt(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} > {}", self.identifier(), other.into())
    }

    fn lt(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} < {}", self.identifier(), other.into())
    }

    fn gte(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} >= {}", self.identifier(), other.into())
    }

    fn lte(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} <= {}", self.identifier(), other.into())
    }

    fn in_list(&self, items: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} IN ({})", self.identifier(), items.into())
    }

    fn is_null(&self) -> Expression {
        expr!("{} IS NULL", self.identifier())
    }

    fn is_not_null(&self) -> Expression {
        expr!("{} IS NOT NULL", self.identifier())
    }

    fn like(&self, pattern: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} LIKE {}", self.identifier(), pattern.into())
    }
}

impl SurrealColumnOperations for &SurrealColumn {
    fn eq(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} = {}", self.identifier(), other.into())
    }

    fn ne(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} != {}", self.identifier(), other.into())
    }

    fn gt(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} > {}", self.identifier(), other.into())
    }

    fn lt(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} < {}", self.identifier(), other.into())
    }

    fn gte(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} >= {}", self.identifier(), other.into())
    }

    fn lte(&self, other: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} <= {}", self.identifier(), other.into())
    }

    fn in_list(&self, items: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} IN ({})", self.identifier(), items.into())
    }

    fn is_null(&self) -> Expression {
        expr!("{} IS NULL", self.identifier())
    }

    fn is_not_null(&self) -> Expression {
        expr!("{} IS NOT NULL", self.identifier())
    }

    fn like(&self, pattern: impl Into<IntoExpressive<Expression>>) -> Expression {
        expr!("{} LIKE {}", self.identifier(), pattern.into())
    }
}

#[cfg(test)]
mod surreal_column_operations_tests {
    use super::*;

    #[test]
    fn test_surreal_column_operations() {
        let col = SurrealColumn::new("age");

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
    fn test_surreal_column_operations_reference() {
        let col = SurrealColumn::new("name");

        // Test with reference
        let expr = col.eq("John");
        assert_eq!(expr.preview(), "name = \"John\"");

        let expr = col.ne("Jane");
        assert_eq!(expr.preview(), "name != \"Jane\"");
    }

    #[test]
    fn test_surreal_column_operations_with_reserved_keyword() {
        let col = SurrealColumn::new("SELECT");

        // Test that reserved keywords get properly escaped
        let expr = col.eq("value");
        assert_eq!(expr.preview(), "⟨SELECT⟩ = \"value\"");
    }
}
