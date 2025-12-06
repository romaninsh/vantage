//! MockQuerySource implementation for unit testing.
//!
//! Provides QuerySource implementation that returns configurable values,
//! useful for testing query execution without actual database connections.
//!
//! # Example
//!
//! ```rust
//! use vantage_expressions::prelude::*;
//! use vantage_expressions::mocks::*;
//! use serde_json::json;
//!
//! #[tokio::test]
//! async fn test_user_service() {
//!     let mock = MockQuerySource::new(json!([
//!         {"id": 1, "name": "Alice"},
//!         {"id": 2, "name": "Bob"}
//!     ]));
//!
//!     let query = expr!("SELECT * FROM users WHERE active = {}", true);
//!     let users = mock.execute(&query).await.unwrap();
//!
//!     assert_eq!(users.as_array().unwrap().len(), 2);
//! }
//! ```

use crate::traits::datasource::{DataSource, ExprDataSource};
use crate::traits::expressive::{DeferredFn, ExpressiveEnum};
use serde_json::Value;
use vantage_core::Result;

/// Mock DataSource that implements QuerySource with configurable return value
#[derive(Debug, Clone)]
pub struct MockExprDataSource {
    value: Value,
}

impl MockExprDataSource {
    /// Create a new MockQuerySource that returns the given value
    pub fn new(value: Value) -> Self {
        Self { value }
    }
}

impl DataSource for MockExprDataSource {}

impl ExprDataSource<serde_json::Value> for MockExprDataSource {
    async fn execute(
        &self,
        _expr: &crate::Expression<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        Ok(self.value.clone())
    }

    fn defer(&self, _expr: crate::Expression<serde_json::Value>) -> DeferredFn<serde_json::Value>
    where
        serde_json::Value: Clone + Send + Sync + 'static,
    {
        let value = self.value.clone();
        DeferredFn::new(move || {
            let value = value.clone();
            Box::pin(async move { Ok(ExpressiveEnum::Scalar(value)) })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr;
    use serde_json::json;

    #[tokio::test]
    async fn test_mock_query_source() {
        let mock = MockExprDataSource::new(json!({"status": "ok"}));
        let expr = expr!("SELECT * FROM anything");

        let result = mock.execute(&expr).await.unwrap();
        assert_eq!(result, json!({"status": "ok"}));
    }

    #[test]
    fn test_mock_query_source_defer() {
        let mock = MockExprDataSource::new(json!({"deferred": "value"}));
        let expr = expr!("SELECT COUNT(*)");

        let _deferred = mock.defer(expr);
        // Just test that it creates without panicking
        assert!(true);
    }
}
