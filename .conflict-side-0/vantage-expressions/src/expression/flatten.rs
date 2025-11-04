//! Flattening functionality for expressions with deferred parameters and nested expressions.
//! This module provides traits and helpers to resolve deferred parameters and flatten nested structures.

use crate::expression::owned::Expression;
use crate::protocol::expressive::ExpressiveEnum;

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
    use crate::expr_any;

    #[test]
    fn test_flatten_nested_expressions() {
        let flattener = ExpressionFlattener::new();

        let nested_expr = expr_any!(String, "Hello {}", "world");
        let main_expr = expr_any!(String, "select {}", (nested_expr));

        let flattened = flattener.flatten(&main_expr);

        assert_eq!(flattened.template, "select Hello {}");
        assert_eq!(flattened.parameters.len(), 1);
    }

    #[test]
    fn test_multiple_nested_expressions() {
        let flattener = ExpressionFlattener::new();

        let greeting = expr_any!(String, "Hello {}", "John");
        let farewell = expr_any!(String, "Goodbye {}", "Jane");
        let main_expr = expr_any!(String, "{} and {}", (greeting), (farewell));

        let flattened = flattener.flatten(&main_expr);

        assert_eq!(flattened.template, "Hello {} and Goodbye {}");
        assert_eq!(flattened.parameters.len(), 2);
    }

    #[test]
    fn test_mixed_parameters() {
        let flattener = ExpressionFlattener::new();

        let nested = expr_any!(String, "count({})", "*");
        let main_expr = expr_any!(
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
