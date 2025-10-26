//! Mock Selectable implementation for testing
//!
//! This module provides a simple mock implementation of the Selectable trait
//! that can be used in DataSource mocks and other testing scenarios.

use crate::Expression;
use crate::protocol::expressive::IntoExpressive;
use crate::protocol::selectable::Selectable;

/// Simple mock implementation of Selectable trait
///
/// This mock doesn't actually build queries - it just provides the interface
/// required by DataSource implementations. For testing purposes, the actual
/// query building logic is typically bypassed in favor of pattern matching
/// on the final expression.
#[derive(Debug, Clone, Default)]
pub struct MockSelect;

impl Selectable<Expression> for MockSelect {
    fn set_source(
        &mut self,
        _source: impl Into<IntoExpressive<Expression>>,
        _alias: Option<String>,
    ) {
        // Mock implementation - no actual query building
    }

    fn add_field(&mut self, _field: impl Into<String>) {
        // Mock implementation - no actual query building
    }

    fn add_expression(&mut self, _expression: Expression, _alias: Option<String>) {
        // Mock implementation - no actual query building
    }

    fn add_where_condition(&mut self, _condition: Expression) {
        // Mock implementation - no actual query building
    }

    fn set_distinct(&mut self, _distinct: bool) {
        // Mock implementation - no actual query building
    }

    fn add_order_by(&mut self, _expression: Expression, _ascending: bool) {
        // Mock implementation - no actual query building
    }

    fn add_group_by(&mut self, _expression: Expression) {
        // Mock implementation - no actual query building
    }

    fn set_limit(&mut self, _limit: Option<i64>, _skip: Option<i64>) {
        // Mock implementation - no actual query building
    }

    fn clear_fields(&mut self) {
        // Mock implementation - no actual query building
    }

    fn clear_where_conditions(&mut self) {
        // Mock implementation - no actual query building
    }

    fn clear_order_by(&mut self) {
        // Mock implementation - no actual query building
    }

    fn clear_group_by(&mut self) {
        // Mock implementation - no actual query building
    }

    fn has_fields(&self) -> bool {
        false // Mock implementation
    }

    fn has_where_conditions(&self) -> bool {
        false // Mock implementation
    }

    fn has_order_by(&self) -> bool {
        false // Mock implementation
    }

    fn has_group_by(&self) -> bool {
        false // Mock implementation
    }

    fn is_distinct(&self) -> bool {
        false // Mock implementation
    }

    fn get_limit(&self) -> Option<i64> {
        None // Mock implementation
    }

    fn get_skip(&self) -> Option<i64> {
        None // Mock implementation
    }
}

impl From<MockSelect> for Expression {
    fn from(_val: MockSelect) -> Self {
        crate::expr!("SELECT * FROM mock")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_select_interface() {
        let mut mock = MockSelect;

        // These should all work without panicking
        mock.set_source("users", None);
        mock.add_field("name");
        mock.add_expression(crate::expr!("age > 18"), Some("adult".to_string()));
        mock.add_where_condition(crate::expr!("active = true"));
        mock.set_distinct(true);
        mock.add_order_by(crate::expr!("name"), true);
        mock.add_group_by(crate::expr!("department"));
        mock.set_limit(Some(10), Some(0));
        mock.clear_fields();
        mock.clear_where_conditions();
        mock.clear_order_by();
        mock.clear_group_by();

        // Test the query methods
        assert!(!mock.has_fields());
        assert!(!mock.has_where_conditions());
        assert!(!mock.has_order_by());
        assert!(!mock.has_group_by());
        assert!(!mock.is_distinct());
        assert_eq!(mock.get_limit(), None);
        assert_eq!(mock.get_skip(), None);

        // Converting to expression should work
        let _expr: Expression = mock.into();
    }
}
