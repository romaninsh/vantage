//! # SurrealDB Column
//!
//! A SurrealDB-specific column implementation that uses Identifier for proper escaping.

use crate::identifier::Identifier;
use crate::operation::Expressive;
use crate::typed_expression::TypedExpression;
use std::collections::HashSet;
use std::marker::PhantomData;
use surreal_client::types::{Any, SurrealType};
use vantage_expressions::Expression;
use vantage_table::{ColumnFlag, ColumnLike};

/// SurrealDB-specific column that renders as an Identifier
#[derive(Debug, Clone)]
pub struct SurrealColumn<Type: SurrealType = Any> {
    name: String,
    alias: Option<String>,
    flags: HashSet<ColumnFlag>,
    _phantom: PhantomData<Type>,
}

impl<T: SurrealType> SurrealColumn<T> {
    /// Create a new SurrealDB column with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: None,
            flags: HashSet::new(),
            _phantom: PhantomData,
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

    /// Get a TypedExpression for this column
    pub fn expr(&self) -> TypedExpression<T> {
        TypedExpression::new(self.identifier().expr())
    }
}

impl SurrealColumn<Any> {
    /// Create a new untyped SurrealDB column with the given name
    pub fn new_any(name: impl Into<String>) -> Self {
        Self::new(name)
    }

    /// Convert an untyped column to a typed column
    pub fn into_type<NewType: SurrealType>(self) -> SurrealColumn<NewType> {
        SurrealColumn {
            name: self.name,
            alias: self.alias,
            flags: self.flags,
            _phantom: PhantomData,
        }
    }
}

impl<T: SurrealType> From<SurrealColumn<T>> for TypedExpression<T> {
    fn from(column: SurrealColumn<T>) -> Self {
        TypedExpression::new(column.identifier().expr())
    }
}

impl<T: SurrealType> From<&SurrealColumn<T>> for TypedExpression<T> {
    fn from(column: &SurrealColumn<T>) -> Self {
        TypedExpression::new(column.identifier().expr())
    }
}

impl<T: SurrealType> From<SurrealColumn<T>> for Expression {
    fn from(column: SurrealColumn<T>) -> Self {
        column.identifier().expr()
    }
}

impl<T: SurrealType> Expressive for SurrealColumn<T> {
    fn expr(&self) -> Expression {
        self.identifier().expr()
    }
}

impl<T: SurrealType> ColumnLike for SurrealColumn<T> {
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl From<&str> for SurrealColumn<Any> {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for SurrealColumn<Any> {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

impl From<vantage_table::Column> for SurrealColumn<Any> {
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
        let col = SurrealColumn::<Any>::new("user_name");
        assert_eq!(col.name(), "user_name");
        assert_eq!(ColumnLike::alias(&col), None);
    }

    #[test]
    fn test_surreal_column_with_alias() {
        let col = SurrealColumn::<Any>::new("user_name").with_alias("name");
        assert_eq!(col.name(), "user_name");
        assert_eq!(ColumnLike::alias(&col), Some("name"));
    }

    #[test]
    fn test_surreal_column_expr() {
        let col = SurrealColumn::<Any>::new("user_name");
        let expr = col.expr();
        assert_eq!(expr.preview(), "user_name");
    }

    #[test]
    fn test_surreal_column_reserved_keyword() {
        let col = SurrealColumn::<Any>::new("SELECT");
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
        assert_eq!(ColumnLike::alias(&surreal_col), Some("test_alias"));
    }

    #[test]
    fn test_surreal_column_with_flags() {
        let col = SurrealColumn::<Any>::new("email").with_flags(&[ColumnFlag::Mandatory]);
        assert!(col.flags().contains(&ColumnFlag::Mandatory));
        assert_eq!(col.flags().len(), 1);
    }

    #[test]
    fn test_surreal_column_no_flags() {
        let col = SurrealColumn::<Any>::new("optional_field");
        assert!(col.flags().is_empty());
    }

    #[test]
    fn test_from_vantage_column_with_flags() {
        let vantage_col = vantage_table::Column::new("test")
            .with_flags(&[ColumnFlag::Mandatory])
            .with_alias("test_alias");
        let surreal_col: SurrealColumn = vantage_col.into();
        assert_eq!(surreal_col.name(), "test");
        assert_eq!(ColumnLike::alias(&surreal_col), Some("test_alias"));
        assert!(surreal_col.flags().contains(&ColumnFlag::Mandatory));
    }
}

#[cfg(test)]
mod column_conversion_tests {
    use super::*;

    #[test]
    fn test_column_to_typed_expression_any() {
        let col = SurrealColumn::<Any>::new("field");
        let typed: TypedExpression<Any> = col.into();
        assert_eq!(typed.type_name(), "any");
    }

    #[test]
    fn test_column_to_typed_expression_i64() {
        let col = SurrealColumn::<i64>::new("age");
        let typed: TypedExpression<i64> = col.into();
        assert_eq!(typed.type_name(), "int");
    }

    #[test]
    fn test_column_to_typed_expression_string() {
        let col = SurrealColumn::<String>::new("name");
        let typed: TypedExpression<String> = col.into();
        assert_eq!(typed.type_name(), "string");
    }

    #[test]
    fn test_column_ref_to_typed_expression() {
        let col = SurrealColumn::<i64>::new("price");
        let typed: TypedExpression<i64> = (&col).into();
        assert_eq!(typed.type_name(), "int");
    }

    #[test]
    fn test_typed_expression_operations_from_column() {
        let col1 = SurrealColumn::<i64>::new("age");
        let col2 = SurrealColumn::<i64>::new("min_age");

        let typed1: TypedExpression<i64> = col1.into();
        let typed2: TypedExpression<i64> = col2.into();

        let expr = typed1.eq(typed2);
        assert_eq!(expr.preview(), "age = min_age");
    }

    #[test]
    fn test_any_column_with_values() {
        let col = SurrealColumn::<Any>::new("field");
        let typed: TypedExpression<Any> = col.into();

        let expr1 = typed.eq_value(42);
        assert_eq!(expr1.preview(), "field = 42");

        let col2 = SurrealColumn::<Any>::new("field");
        let typed2: TypedExpression<Any> = col2.into();
        let expr2 = typed2.eq_value("hello");
        assert_eq!(expr2.preview(), "field = \"hello\"");
    }

    #[test]
    fn test_column_direct_operations() {
        use crate::prelude::*;
        let id_col = SurrealColumn::<String>::new("id");
        let expr = id_col.eq("user:123");
        assert_eq!(expr.preview(), "id = \"user:123\"");
    }
}
