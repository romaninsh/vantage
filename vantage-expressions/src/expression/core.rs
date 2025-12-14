//! Expressions that own their parameters. While any type can be a parameter,
//! most usual is to use:
//!  - serde_json::Value
//!  - ciborium::Value
//!  - bson::Bson
//!  - toml::Value
//!  - rmpv::Value (MessagePack)
//!
//! # Examples
//!
//! Basic [`Expression`] creation owns parameters.
//! ```rust
//! use vantage_expressions::{Expression, traits::expressive::ExpressiveEnum};
//!
//! let expr = Expression::new(
//!     "SELECT * FROM users WHERE age > {}",
//!     vec![ExpressiveEnum::Scalar(21)]
//! );
//! ```
//!
//! Expressions consists of [`ExpressiveEnum`] and can be nested.
//! ```rust
//! use vantage_expressions::expr;
//!
//! let where_clause = expr!("age > {} AND status = {}", 21, "active");
//! let query = expr!("SELECT * FROM users WHERE {}", (where_clause));
//! ```
//!
//! Storing parts of expressions, then combining them with [`Expression::from_vec`]
//! is a standard pattern used by query builders
//! (See [Selectable trait](crate::traits::selectable::Selectable)):
//! ```rust
//! use vantage_expressions::{expr, Expression};
//!
//! let mut conditions = Vec::new();
//! conditions.push(expr!("age >= {}", 18));
//! conditions.push(expr!("status = {}", "active"));
//!
//! let where_clause = Expression::from_vec(conditions, " AND ");
//! let query = expr!("SELECT * FROM users WHERE {}", (where_clause));
//! ```
//!
//! Ultimately expression engine is designed to integrate multiple DataSources
//! and allow construction of queries without async need. Consider cross-database
//! refactoring where user table migrates to an external API using [DeferredFn](crate::traits::expressive::DeferredFn).
//!
//! The example below demonstrates deferred execution with testing support from the
//! [`crate::mocks`] module:
//!
//! ```rust,ignore
//! use vantage_expressions::{expr, mocks::mockbuilder, traits::expressive::DeferredFn};
//! use serde_json::json;
//!
//! # tokio_test::block_on(async {
//! // API call that fetches user IDs asynchronously
//! async fn get_user_ids() -> vantage_core::Result<serde_json::Value> {
//!     // Simulate API call - fetch from external service
//!     Ok(json!([1, 2, 3, 4, 5]))
//! }
//!
//! // Set up mock to handle the flattened query after deferred execution
//! let db = mockbuilder::new()
//!     .with_flattening()
//!     .on_exact_select("SELECT * FROM orders WHERE user_id = ANY([1,2,3,4,5])", json!([
//!         {"id": 1, "user_id": 2, "amount": 99.99}
//!     ]));
//!
//! // Build query synchronously - no async needed here!
//! let query = expr!("SELECT * FROM orders WHERE user_id = ANY({})",
//!                  { DeferredFn::from_fn(get_user_ids) });
//!
//! // Execute the query - API call happens automatically during execution
//! let orders = db.execute(&query).await.unwrap();
//! assert_eq!(orders.as_array().unwrap().len(), 1);
//! assert_eq!(orders[0]["amount"], 99.99);
//! assert_eq!(query.preview(), "SELECT * FROM orders WHERE user_id = ANY(**deferred())");
//! # });
//! ```

use crate::traits::expressive::{Expressive, ExpressiveEnum};

/// Owned expression contains template and Vec of IntoExpressive parameters
#[derive(Clone)]
pub struct Expression<T> {
    pub template: String,
    pub parameters: Vec<ExpressiveEnum<T>>,
}

impl<T: Clone> Expressive<T> for Expression<T> {
    fn expr(&self) -> Expression<T> {
        self.clone()
    }
}

impl<T> From<Expression<T>> for ExpressiveEnum<T> {
    fn from(expr: Expression<T>) -> Self {
        ExpressiveEnum::Nested(expr)
    }
}

impl<T: std::fmt::Debug + std::fmt::Display> std::fmt::Debug for Expression<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.preview())
    }
}

impl<T> Expression<T> {
    /// Create a new owned expression with template and parameters
    pub fn new(template: impl Into<String>, parameters: Vec<ExpressiveEnum<T>>) -> Self {
        Self {
            template: template.into(),
            parameters,
        }
    }

    /// Create expression from vector of expressions and a delimiter
    ///
    /// See the [module-level documentation](crate::expression::expression) for examples.
    pub fn from_vec(vec: Vec<Expression<T>>, delimiter: &str) -> Self {
        let template = vec
            .iter()
            .map(|_| "{}")
            .collect::<Vec<&str>>()
            .join(delimiter);

        let parameters = vec.into_iter().map(ExpressiveEnum::nested).collect();

        Self {
            template,
            parameters,
        }
    }
}

impl<T: std::fmt::Display + std::fmt::Debug> Expression<T> {
    pub fn preview(&self) -> String {
        let mut preview = self.template.clone();
        for param in &self.parameters {
            let param_str = param.preview();
            preview = preview.replacen("{}", &param_str, 1);
        }
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expression_basic() {
        let expr = Expression::new(
            "SELECT * FROM table WHERE id = {}",
            vec![ExpressiveEnum::Scalar(42)],
        );
        assert_eq!(expr.template, "SELECT * FROM table WHERE id = {}");
        assert_eq!(expr.parameters.len(), 1);
        assert_eq!(expr.preview(), "SELECT * FROM table WHERE id = 42");
    }
}
