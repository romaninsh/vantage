//! Flattening turns nested expression hierarchies into one flat template, so
//! database drivers can bind the parameters safely.
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

use crate::expression::core::Expression;
use crate::traits::expressive::ExpressiveEnum;

/// Flattens nested expressions into their parent template.
pub trait Flatten<T> {
    /// Flatten nested expressions; deferred parameters stay deferred (see
    /// [`Expression::resolve_deferred`]).
    fn flatten(&self, expr: &T) -> T;

    /// Flatten nested expressions into the parent template
    fn flatten_nested(&self, expr: &T) -> T;
}

/// Default [`Flatten`] implementation for [`Expression`].
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
        // Deferred parameters are resolved asynchronously by the execute path
        // (`Expression::resolve_deferred`), not by this sync flattener.
        self.flatten_nested(expr)
    }

    fn flatten_nested(&self, expr: &Expression<T>) -> Expression<T> {
        let mut result = expr.clone();
        loop {
            let mut final_template = String::new();
            let mut final_params = Vec::new();
            let mut had_nested = false;
            let template_parts = result.template.split("{}");

            let mut template_iter = template_parts.into_iter();
            final_template.push_str(template_iter.next().unwrap_or(""));

            for param in &result.parameters {
                match param {
                    ExpressiveEnum::Nested(nested_expr) => {
                        had_nested = true;
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

            result = Expression {
                template: final_template,
                parameters: final_params,
            };

            if !had_nested {
                break;
            }
        }
        result
    }
}
