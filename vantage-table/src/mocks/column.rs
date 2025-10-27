//! Mock column implementation for testing
//!
//! Provides a simple column implementation that can be used across all mock DataSources.

use crate::Column;
use crate::ColumnFlag;
use crate::ColumnLike;
use crate::Expression;
use std::collections::HashSet;
use vantage_expressions::IntoExpressive;
use vantage_expressions::expr;

/// Simple column implementation for testing mocks
#[derive(Debug, Clone)]
pub struct MockColumn {
    name: String,
    flags: HashSet<ColumnFlag>,
}

impl MockColumn {
    /// Create a new mock column with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            flags: HashSet::new(),
        }
    }
}

impl ColumnLike for MockColumn {
    fn name(&self) -> &str {
        &self.name
    }

    fn alias(&self) -> Option<&str> {
        None
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

impl From<MockColumn> for IntoExpressive<Expression> {
    fn from(val: MockColumn) -> Self {
        IntoExpressive::nested(val.expr())
    }
}

impl From<&MockColumn> for IntoExpressive<Expression> {
    fn from(val: &MockColumn) -> Self {
        IntoExpressive::nested(val.expr())
    }
}

impl From<&str> for MockColumn {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for MockColumn {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

impl From<Column> for MockColumn {
    fn from(col: Column) -> Self {
        Self {
            name: col.name().to_string(),
            flags: col.flags().clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_column_basic() {
        let col = MockColumn::new("test_column");
        assert_eq!(col.name(), "test_column");
        assert_eq!(col.alias(), None);
    }

    #[test]
    fn test_mock_column_no_alias() {
        let col = MockColumn::new("original_name");
        assert_eq!(col.name(), "original_name");
        assert_eq!(col.alias(), None);
    }

    #[test]
    fn test_mock_column_expr() {
        let col = MockColumn::new("test_field");
        let expr = col.expr();
        assert_eq!(expr.preview(), "test_field");
    }

    #[test]
    fn test_from_str() {
        let col: MockColumn = "email".into();
        assert_eq!(col.name(), "email");
        assert_eq!(col.alias(), None);
    }

    #[test]
    fn test_from_string() {
        let col: MockColumn = "name".to_string().into();
        assert_eq!(col.name(), "name");
        assert_eq!(col.alias(), None);
    }
}
