//! StaticDataSource implementation
//!
//! Always returns the same static value regardless of query.
//!
//! ## Examples
//!
//! ### Single object result:
//! ```rust
//! use vantage_expressions::mocks::StaticDataSource;
//! use serde_json::json;
//!
//! let mock = StaticDataSource::new(json!({"status": "ok", "count": 42}));
//! // Any query returns {"status": "ok", "count": 42}
//! ```
//!
//! ### Array of objects result:
//! ```rust
//! use vantage_expressions::mocks::StaticDataSource;
//! use serde_json::json;
//!
//! let mock = StaticDataSource::new(json!([
//!     {"id": 1, "name": "Alice", "email": "alice@example.com"},
//!     {"id": 2, "name": "Bob", "email": "bob@example.com"}
//! ]));
//! // Any query returns the array of users
//! ```

use crate::QuerySource;
use crate::SelectSource;
use crate::mocks::selectable::MockSelect;
use crate::protocol::datasource::DataSource;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use vantage_core::Result;

/// Mock DataSource that always returns the same static value
#[derive(Debug, Clone)]
pub struct StaticDataSource {
    value: Value,
}

impl StaticDataSource {
    /// Create a new StaticDataSource that always returns the given value
    pub fn new(value: Value) -> Self {
        Self { value }
    }
}

impl DataSource for StaticDataSource {}
impl QuerySource<serde_json::Value> for StaticDataSource {
    async fn execute(&self, _expr: &crate::Expression<serde_json::Value>) -> serde_json::Value {
        self.value.clone()
    }

    fn defer(
        &self,
        _expr: crate::Expression<serde_json::Value>,
    ) -> impl Fn() -> Pin<Box<dyn Future<Output = serde_json::Value> + Send>> + Send + Sync + 'static
    {
        let value = self.value.clone();
        move || {
            let value = value.clone();
            Box::pin(async move { value })
        }
    }
}

impl SelectSource<serde_json::Value> for StaticDataSource {
    type Select = MockSelect;

    fn select(&self) -> Self::Select {
        MockSelect
    }

    async fn execute_select(&self, _select: &Self::Select) -> Result<Vec<serde_json::Value>> {
        // Return the stored JSON value as Vec<Value>
        if let Value::Array(arr) = &self.value {
            Ok(arr.clone())
        } else {
            Ok(vec![])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr;
    use serde_json::json;

    #[tokio::test]
    async fn test_static() {
        let mock = StaticDataSource::new(json!({"status": "ok"}));
        let expr = expr!("SELECT * FROM anything");

        let result = mock.execute(&expr).await;
        assert_eq!(result, json!({"status": "ok"}));
    }

    #[tokio::test]
    async fn test_static_array() {
        let mock = StaticDataSource::new(json!([
            {"id": 1, "name": "Alice"},
            {"id": 2, "name": "Bob"}
        ]));
        let expr = expr!("SELECT * FROM users");

        let result = mock.execute(&expr).await;
        assert_eq!(result[0]["name"], "Alice");
        assert_eq!(result[1]["name"], "Bob");
        assert_eq!(result.as_array().unwrap().len(), 2);
    }
}
