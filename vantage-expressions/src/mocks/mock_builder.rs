//! MockBuilder: a mock datasource that matches exact query text, optionally
//! after flattening, and returns configured responses.
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
use crate::traits::datasource::{DataSource, ExprDataSource, SelectableDataSource};
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

impl ExprDataSource<Value> for MockBuilder {
    async fn execute(&self, expr: &Expression<Value>) -> Result<Value> {
        let resolved_expr = expr.resolve_deferred().await?;
        let processed_expr = self.process_expression(&resolved_expr);
        let query_str = processed_expr.preview();

        match self.find_matching_response(&query_str) {
            Some(response) => Ok(response),
            None => Err(vantage_core::error!(
                "No matching pattern found for query",
                query = query_str
            )),
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
                    )),
                }
            })
        })
    }
}

impl SelectableDataSource<Value> for MockBuilder {
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
