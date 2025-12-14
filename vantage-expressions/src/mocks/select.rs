//! Mock Select implementation for testing
//!
//! This module provides a minimalistic mock implementation of the Selectable trait
//! that uses the expr! macro pattern for building queries.

use crate::traits::expressive::ExpressiveEnum;
use crate::traits::selectable::{Selectable, SourceRef};
use crate::{Expression, expr};

/// Minimalistic mock implementation of Selectable trait
///
/// This mock tracks basic query components and uses expr! macro to build
/// SQL-like queries for testing purposes.
///
/// ## Examples
///
/// ```rust
/// use vantage_expressions::mocks::select::MockSelect;
/// use vantage_expressions::traits::selectable::Selectable;
///
/// let mut select = MockSelect::new();
/// select.set_source("users", None);
/// select.add_field("name");
/// select.add_field("email");
///
/// let query: vantage_expressions::Expression<serde_json::Value> = select.into();
/// // Results in "SELECT name, email FROM users"
/// ```
#[derive(Debug, Clone, Default)]
pub struct MockSelect {
    source: Option<String>,
    fields: Vec<String>,
    where_conditions: Vec<Expression<serde_json::Value>>,
    order_by: Vec<(Expression<serde_json::Value>, bool)>,
    distinct: bool,
    limit: Option<i64>,
    skip: Option<i64>,
}

impl MockSelect {
    /// Create a new MockSelect
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the source table name
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// Get the list of field names
    pub fn fields(&self) -> &[String] {
        &self.fields
    }

    /// Get the where conditions
    pub fn where_conditions(&self) -> &[Expression<serde_json::Value>] {
        &self.where_conditions
    }

    fn render(&self) -> Expression<serde_json::Value> {
        let mut query = String::from("SELECT");

        if self.distinct {
            query.push_str(" DISTINCT");
        }

        // Add fields
        if self.fields.is_empty() {
            query.push_str(" *");
        } else {
            query.push(' ');
            query.push_str(&self.fields.join(", "));
        }

        // Add FROM clause
        if let Some(source) = &self.source {
            query.push_str(" FROM ");
            query.push_str(source);
        }

        // Add WHERE clause
        if !self.where_conditions.is_empty() {
            query.push_str(" WHERE ");
            let conditions: Vec<String> = self
                .where_conditions
                .iter()
                .map(|c| c.template.clone())
                .collect();
            query.push_str(&conditions.join(" AND "));
        }

        // Add ORDER BY clause
        if !self.order_by.is_empty() {
            query.push_str(" ORDER BY ");
            let orders: Vec<String> = self
                .order_by
                .iter()
                .map(|(e, asc)| {
                    if *asc {
                        format!("{} ASC", e.template)
                    } else {
                        format!("{} DESC", e.template)
                    }
                })
                .collect();
            query.push_str(&orders.join(", "));
        }

        // Add LIMIT clause
        match (self.limit, self.skip) {
            (Some(limit), Some(skip)) => {
                query.push_str(&format!(" LIMIT {} OFFSET {}", limit, skip))
            }
            (Some(limit), None) => query.push_str(&format!(" LIMIT {}", limit)),
            (None, Some(skip)) => query.push_str(&format!(" OFFSET {}", skip)),
            (None, None) => {}
        }

        Expression::new(query, vec![])
    }
}

impl Selectable<serde_json::Value> for MockSelect {
    fn set_source(
        &mut self,
        source: impl Into<SourceRef<serde_json::Value>>,
        _alias: Option<String>,
    ) {
        match source.into().into_expressive_enum() {
            ExpressiveEnum::Scalar(serde_json::Value::String(s)) => {
                self.source = Some(s);
            }
            _ => panic!("You may only use string source with this mock"),
        }
    }

    fn add_field(&mut self, field: impl Into<String>) {
        self.fields.push(field.into());
    }

    fn add_expression(
        &mut self,
        _expression: Expression<serde_json::Value>,
        _alias: Option<String>,
    ) {
        panic!("You may only use field() wihis mock")
    }

    fn add_where_condition(&mut self, condition: Expression<serde_json::Value>) {
        self.where_conditions.push(condition);
    }

    fn set_distinct(&mut self, distinct: bool) {
        self.distinct = distinct;
    }

    fn add_order_by(&mut self, expression: Expression<serde_json::Value>, ascending: bool) {
        self.order_by.push((expression, ascending));
    }

    fn add_group_by(&mut self, _expression: Expression<serde_json::Value>) {
        // Not implemented in minimal version
    }

    fn set_limit(&mut self, limit: Option<i64>, skip: Option<i64>) {
        self.limit = limit;
        self.skip = skip;
    }

    fn clear_fields(&mut self) {
        self.fields.clear();
    }

    fn clear_where_conditions(&mut self) {
        self.where_conditions.clear();
    }

    fn clear_order_by(&mut self) {
        self.order_by.clear();
    }

    fn clear_group_by(&mut self) {
        // Not implemented in minimal version
    }

    fn has_fields(&self) -> bool {
        !self.fields.is_empty()
    }

    fn has_where_conditions(&self) -> bool {
        !self.where_conditions.is_empty()
    }

    fn has_order_by(&self) -> bool {
        !self.order_by.is_empty()
    }

    fn has_group_by(&self) -> bool {
        false // Not implemented in minimal version
    }

    fn is_distinct(&self) -> bool {
        self.distinct
    }

    fn get_limit(&self) -> Option<i64> {
        self.limit
    }

    fn get_skip(&self) -> Option<i64> {
        self.skip
    }

    fn as_count(&self) -> Expression<serde_json::Value> {
        let source = self.source.as_ref().unwrap().as_str();
        expr!("SELECT COUNT(*) FROM {}", source)
    }

    fn as_sum(&self, column: Expression<serde_json::Value>) -> Expression<serde_json::Value> {
        let source = self.source.as_ref().unwrap().as_str();
        expr!("SELECT SUM({}) FROM {}", (column), source)
    }
}

impl From<MockSelect> for Expression<serde_json::Value> {
    fn from(val: MockSelect) -> Self {
        val.render()
    }
}

impl crate::traits::expressive::Expressive<serde_json::Value> for MockSelect {
    fn expr(&self) -> Expression<serde_json::Value> {
        self.render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr;

    #[test]
    fn test_mock_select_basic() {
        let mut select = MockSelect::new();
        select.set_source("users", None);

        let query: Expression<serde_json::Value> = select.into();
        assert_eq!(query.preview(), "SELECT * FROM users");
    }

    #[test]
    fn test_mock_select_with_fields() {
        let mut select = MockSelect::new();
        select.set_source("users", None);
        select.add_field("name");
        select.add_field("email");

        let query: Expression<serde_json::Value> = select.into();
        assert_eq!(query.preview(), "SELECT name, email FROM users");
    }

    #[test]
    fn test_mock_select_with_conditions() {
        let mut select = MockSelect::new();
        select.set_source("users", None);
        select.add_field("name");
        select.add_where_condition(expr!("age > 18"));

        let query: Expression<serde_json::Value> = select.into();
        assert_eq!(query.preview(), "SELECT name FROM users WHERE age > 18");
    }
}
