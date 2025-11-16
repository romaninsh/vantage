//! MockBuilder for pattern-based query testing with flattening support.
//!
//! Provides a builder pattern for creating mock data sources that can match
//! specific query patterns and return configured responses. Useful for testing
//! complex query scenarios without actual database connections.
//!
//! # Examples
//!
//! Basic pattern matching:
//! ```rust
//! use vantage_expressions::prelude::*;
//! use vantage_expressions::mocks::mockbuilder;
//! use serde_json::json;
//!
//! # tokio_test::block_on(async {
//! let mock = mockbuilder::new()
//!     .on_exact_select("SELECT * FROM users", json!([
//!         {"id": 1, "name": "Alice"},
//!         {"id": 2, "name": "Bob"}
//!     ]));
//!
//! let query = expr!("SELECT * FROM users");
//! let result = mock.execute(&query).await.unwrap();
//! assert_eq!(result.as_array().unwrap().len(), 2);
//! # });
//! ```
//!
//! Multiple patterns with flattening:
//! ```rust
//! use vantage_expressions::prelude::*;
//! use vantage_expressions::mocks::mockbuilder;
//! use serde_json::json;
//!
//! # tokio_test::block_on(async {
//! let mock = mockbuilder::new()
//!     .with_flattening()
//!     .on_exact_select("SELECT COUNT(*) FROM orders WHERE user_id IN (SELECT id FROM users WHERE active = true)", json!(42))
//!     .on_exact_select("SELECT * FROM products", json!([
//!         {"id": "prod1", "name": "Widget"}
//!     ]));
//!
//! let user_subquery = expr!("SELECT id FROM users WHERE active = {}", true);
//! let nested_query = expr!("SELECT COUNT(*) FROM orders WHERE user_id IN ({})", (user_subquery));
//! let result = mock.execute(&nested_query).await.unwrap();
//! assert_eq!(result, json!(42));
//! # });
//! ```

use crate::Expression;
use crate::expression::flatten::{ExpressionFlattener, Flatten};
use crate::mocks::select::MockSelect;
use crate::traits::datasource::{DataSource, QuerySource, SelectSource};
use crate::traits::expressive::{DeferredFn, ExpressiveEnum};
use serde_json::Value;
use std::collections::HashMap;
use vantage_core::Result;

/// Mock builder for creating pattern-based mock data sources
#[derive(Debug, Clone)]
pub struct MockBuilder {
    patterns: HashMap<String, Value>,
    flatten_expressions: bool,
    flattener: ExpressionFlattener,
}

impl MockBuilder {
    /// Create a new mock builder
    pub fn new() -> Self {
        Self {
            patterns: HashMap::new(),
            flatten_expressions: false,
            flattener: ExpressionFlattener::new(),
        }
    }

    /// Enable expression flattening before pattern matching
    pub fn with_flattening(mut self) -> Self {
        self.flatten_expressions = true;
        self
    }

    /// Add an exact pattern match for select queries
    pub fn on_exact_select(mut self, pattern: impl Into<String>, response: Value) -> Self {
        self.patterns.insert(pattern.into(), response);
        self
    }

    fn process_expression(&self, expr: &Expression<Value>) -> Expression<Value> {
        if self.flatten_expressions {
            self.flattener.flatten(expr)
        } else {
            expr.clone()
        }
    }

    async fn resolve_deferred_expression(
        &self,
        expr: &Expression<Value>,
    ) -> Result<Expression<Value>> {
        let mut resolved_params = Vec::new();

        for param in &expr.parameters {
            match param {
                ExpressiveEnum::Deferred(deferred_fn) => {
                    let result = deferred_fn.call().await?;
                    resolved_params.push(result);
                }
                other => {
                    resolved_params.push(other.clone());
                }
            }
        }

        Ok(Expression::new(expr.template.clone(), resolved_params))
    }

    fn find_matching_response(&self, query: &str) -> Option<Value> {
        self.patterns.get(query).cloned()
    }
}

impl Default for MockBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for MockBuilder {}

impl QuerySource<Value> for MockBuilder {
    async fn execute(&self, expr: &Expression<Value>) -> Result<Value> {
        // First resolve any deferred functions
        let resolved_expr = self.resolve_deferred_expression(expr).await?;

        // Then process (flatten if enabled)
        let processed_expr = self.process_expression(&resolved_expr);
        let query_str = processed_expr.preview();

        match self.find_matching_response(&query_str) {
            Some(response) => Ok(response),
            None => Err(vantage_core::error!(
                "No matching pattern found for query",
                query = query_str
            )
            .into()),
        }
    }

    fn defer(&self, expr: Expression<Value>) -> DeferredFn<Value>
    where
        Value: Clone + Send + Sync + 'static,
    {
        let processed_expr = self.process_expression(&expr);
        let query_str = processed_expr.preview();
        let response = self.find_matching_response(&query_str);

        DeferredFn::new(move || {
            let response = response.clone();
            let query_str = query_str.clone();
            Box::pin(async move {
                match response {
                    Some(value) => Ok(ExpressiveEnum::Scalar(value)),
                    None => Err(vantage_core::error!(
                        "No matching pattern found for deferred query",
                        query = query_str
                    )
                    .into()),
                }
            })
        })
    }
}

impl SelectSource<Value> for MockBuilder {
    type Select = MockSelect;

    fn select(&self) -> Self::Select {
        MockSelect::new()
    }

    async fn execute_select(&self, select: &Self::Select) -> Result<Vec<Value>> {
        use crate::traits::expressive::Expressive;
        let expr = select.expr();
        let result = self.execute(&expr).await?;

        match result {
            Value::Array(arr) => Ok(arr),
            single_value => Ok(vec![single_value]),
        }
    }
}

/// Create a new mock builder
pub fn new() -> MockBuilder {
    MockBuilder::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr;
    use serde_json::json;

    #[tokio::test]
    async fn test_exact_pattern_matching() {
        let mock = new().on_exact_select(
            "SELECT * FROM users",
            json!([
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"}
            ]),
        );

        let query = expr!("SELECT * FROM users");
        let result = mock.execute(&query).await.unwrap();
        assert_eq!(result.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_multiple_patterns() {
        let mock = new()
            .on_exact_select("SELECT COUNT(*) FROM orders", json!(42))
            .on_exact_select(
                "SELECT * FROM products",
                json!([
                    {"id": "prod1", "name": "Widget"}
                ]),
            );

        let count_query = expr!("SELECT COUNT(*) FROM orders");
        let count_result = mock.execute(&count_query).await.unwrap();
        assert_eq!(count_result, json!(42));

        let products_query = expr!("SELECT * FROM products");
        let products_result = mock.execute(&products_query).await.unwrap();
        assert_eq!(products_result.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_flattening() {
        let mock = new().with_flattening().on_exact_select(
            "SELECT * FROM orders WHERE user_id = 123",
            json!([
                {"id": 1, "user_id": 123, "amount": 99.99}
            ]),
        );

        let nested_query = expr!("SELECT * FROM orders WHERE user_id = {}", 123);
        let result = mock.execute(&nested_query).await.unwrap();
        assert_eq!(result.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_no_matching_pattern() {
        let mock = new().on_exact_select("SELECT * FROM users", json!([]));

        let query = expr!("SELECT * FROM products");
        let result = mock.execute(&query).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_defer() {
        let mock = new().on_exact_select("SELECT COUNT(*)", json!(5));

        let query = expr!("SELECT COUNT(*)");
        let _deferred = mock.defer(query);
        // Just test that it creates without panicking
        assert!(true);
    }

    #[tokio::test]
    async fn test_select_source() {
        use crate::traits::selectable::Selectable;

        let mock = new().on_exact_select(
            "SELECT * FROM products",
            json!([
                {"id": "prod1", "name": "Widget"}
            ]),
        );

        let mut select = mock.select();
        select.set_source("products", None);

        let results = mock.execute_select(&select).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["name"], "Widget");
    }
}
