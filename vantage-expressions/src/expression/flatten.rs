//! Flattening functionality for expressions with deferred parameters and nested expressions.
//! This module provides traits and helpers to resolve deferred parameters and flatten nested structures.
//!
//! Flattening is essential for database query preparation, converting nested expression hierarchies
//! into flat templates suitable for parameter binding. Database drivers can then safely bind
//! parameters without SQL injection risks.
//!
//! # Example: SurrealDB Parameter Binding
//!
//! ```rust
//! use vantage_expressions::prelude::*;
//!
//! // Build nested query structure
//! let where_clause = expr!("age > {} AND department = {}", 25, "engineering");
//! let select_query = expr!("SELECT * FROM employees WHERE {}", (where_clause));
//!
//! // Flatten for database execution
//! let flattener = ExpressionFlattener::new();
//! let flattened = flattener.flatten(&select_query);
//!
//! // Result: template = "SELECT * FROM employees WHERE age > {} AND department = {}"
//! //         parameters = [25, "engineering"]
//! // Database driver converts {} to $_arg1, $_arg2 for safe parameter binding
//! ```
//!
//! This pattern allows complex query composition while maintaining parameter safety during execution.

use crate::expression::expression::Expression;
use crate::traits::expressive::ExpressiveEnum;

/// Trait for flattening expressions by resolving deferred parameters and nested expressions
pub trait Flatten<T> {
    /// Flatten an expression by resolving all deferred parameters and nested expressions
    fn flatten(&self, expr: &T) -> T;

    /// Resolve deferred parameters in the expression
    fn resolve_deferred(&self, expr: &T) -> T;

    /// Flatten nested expressions into the parent template
    fn flatten_nested(&self, expr: &T) -> T;
}

/// Default implementation for Expression flattening
#[derive(Debug, Clone)]
pub struct ExpressionFlattener;

impl ExpressionFlattener {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExpressionFlattener {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> Flatten<Expression<T>> for ExpressionFlattener {
    fn flatten(&self, expr: &Expression<T>) -> Expression<T> {
        let resolved = self.resolve_deferred(expr);
        self.flatten_nested(&resolved)
    }

    fn resolve_deferred(&self, expr: &Expression<T>) -> Expression<T> {
        // Note: This is a sync implementation that doesn't actually execute deferred closures
        // For testing purposes, deferred parameters are left as-is
        // In real usage, this would be handled by the DataSource execute method
        expr.clone()
    }

    fn flatten_nested(&self, expr: &Expression<T>) -> Expression<T> {
        let mut final_template = String::new();
        let mut final_params = Vec::new();
        let template_parts = expr.template.split("{}");

        let mut template_iter = template_parts.into_iter();
        final_template.push_str(template_iter.next().unwrap_or(""));

        for param in &expr.parameters {
            match param {
                ExpressiveEnum::Nested(nested_expr) => {
                    final_template.push_str(&nested_expr.template);
                    final_params.extend(nested_expr.parameters.clone());
                }
                other => {
                    final_template.push_str("{}");
                    final_params.push(other.clone());
                }
            }
            final_template.push_str(template_iter.next().unwrap_or(""));
        }

        Expression {
            template: final_template,
            parameters: final_params,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr_as;

    #[test]
    fn test_flatten_nested_expressions() {
        let flattener = ExpressionFlattener::new();

        let nested_expr = expr_as!(String, "Hello {}", "world");
        let main_expr = expr_as!(String, "select {}", (nested_expr));

        let flattened = flattener.flatten(&main_expr);

        assert_eq!(flattened.template, "select Hello {}");
        assert_eq!(flattened.parameters.len(), 1);
    }

    #[test]
    fn test_multiple_nested_expressions() {
        let flattener = ExpressionFlattener::new();

        let greeting = expr_as!(String, "Hello {}", "John");
        let farewell = expr_as!(String, "Goodbye {}", "Jane");
        let main_expr = expr_as!(String, "{} and {}", (greeting), (farewell));

        let flattened = flattener.flatten(&main_expr);

        assert_eq!(flattened.template, "Hello {} and Goodbye {}");
        assert_eq!(flattened.parameters.len(), 2);
    }

    #[test]
    fn test_mixed_parameters() {
        let flattener = ExpressionFlattener::new();

        let nested = expr_as!(String, "count({})", "*");
        let main_expr = expr_as!(
            String,
            "SELECT {} FROM users WHERE age > {}",
            (nested),
            "25"
        );

        let flattened = flattener.flatten(&main_expr);

        assert_eq!(
            flattened.template,
            "SELECT count({}) FROM users WHERE age > {}"
        );
        assert_eq!(flattened.parameters.len(), 2);
    }
}
