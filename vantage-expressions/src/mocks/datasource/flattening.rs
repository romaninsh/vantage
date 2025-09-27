//! FlatteningPatternDataSource implementation
//!
//! Maps query patterns to specific responses with expression flattening support.

use crate::Expression;
use crate::IntoExpressive;
use crate::QuerySource;
use crate::expression::flatten::{ExpressionFlattener, Flatten};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// Expression PatternDataSource with flattening enabled
#[derive(Debug, Clone)]
pub struct FlatteningPatternDataSource {
    patterns: HashMap<String, Value>,
}

impl FlatteningPatternDataSource {
    /// Create a new FlatteningPatternDataSource with no patterns
    pub fn new() -> Self {
        Self {
            patterns: HashMap::new(),
        }
    }

    /// Add a pattern that will match queries exactly
    pub fn with_pattern(mut self, query: impl Into<String>, value: Value) -> Self {
        self.patterns.insert(query.into(), value);
        self
    }

    /// Find exact match for a query
    fn find_match(&self, query: &str) -> Value {
        self.patterns
            .get(query)
            .cloned()
            .unwrap_or_else(|| panic!("No pattern found for query: {}", query))
    }

    /// Execute deferred parameters and flatten nested expressions recursively
    async fn execute_and_flatten_expression(&self, expr: &Expression) -> Expression {
        let mut expr = expr.clone();
        let flattener = ExpressionFlattener::new();
        let mut max_iterations = 10; // Prevent infinite loops

        // Keep processing until no more deferred parameters exist
        loop {
            let mut has_deferred = false;

            // Execute all deferred parameters at current level
            for param in &mut expr.parameters {
                if let IntoExpressive::Deferred(f) = param {
                    *param = f().await;
                    has_deferred = true;
                }
            }

            // Use Flatten trait to flatten nested expressions
            expr = flattener.flatten_nested(&expr);

            // Check if there are still deferred parameters after flattening
            let still_has_deferred = expr
                .parameters
                .iter()
                .any(|p| matches!(p, IntoExpressive::Deferred(_)));

            if !has_deferred && !still_has_deferred {
                break;
            }

            max_iterations -= 1;
            if max_iterations == 0 {
                panic!("Maximum recursion depth reached in expression flattening");
            }
        }

        expr
    }
}

impl Default for FlatteningPatternDataSource {
    fn default() -> Self {
        Self::new()
    }
}

impl QuerySource<Expression> for FlatteningPatternDataSource {
    // type Column = crate::mocks::MockColumn;

    // fn select(&self) -> impl Selectable {
    //     crate::mocks::selectable::MockSelect
    // }

    async fn execute(&self, expr: &Expression) -> Value {
        let processed_expr = self.execute_and_flatten_expression(expr).await;
        let query = processed_expr.preview();
        self.find_match(&query)
    }

    fn defer(
        &self,
        expr: Expression,
    ) -> impl Fn() -> Pin<Box<dyn Future<Output = Value> + Send>> + Send + Sync + 'static {
        let mock = self.clone();
        move || {
            let mock = mock.clone();
            let expr = expr.clone();
            Box::pin(async move {
                let processed_expr = mock.execute_and_flatten_expression(&expr).await;
                let query = processed_expr.preview();
                mock.find_match(&query)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr;
    use serde_json::json;

    #[tokio::test]
    async fn test_flattening_pattern() {
        let mock = FlatteningPatternDataSource::new()
            .with_pattern("hello \"world\"", json!("greeting_world"));

        let greeting = expr!("hello {}", "world");
        let result = mock.execute(&greeting).await;
        assert_eq!(result, json!("greeting_world"));
    }
}
