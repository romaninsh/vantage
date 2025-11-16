//! MockSelectSource implementation for unit testing.
//!
//! Provides SelectSource implementation that returns configurable values,
//! useful for testing select query builders without actual database connections.
//!
//! # Example
//!
//! ```rust
//! use vantage_expressions::prelude::*;
//! use vantage_expressions::mocks::*;
//! use serde_json::json;
//!
//! #[tokio::test]
//! async fn test_product_repository() {
//!     let mock = MockSelectSource::new(json!([
//!         {"id": 1, "name": "Laptop", "price": 999.99},
//!         {"id": 2, "name": "Mouse", "price": 29.99}
//!     ]));
//!
//!     let mut select = mock.select();
//!     select.set_source("products", None);
//!     select.add_field("name");
//!     select.add_field("price");
//!
//!     let products = mock.execute_select(&select).await.unwrap();
//!     assert_eq!(products.len(), 2);
//!     assert_eq!(products[0]["name"], "Laptop");
//! }
//! ```

use crate::mocks::select::MockSelect;
use crate::traits::datasource::{DataSource, SelectSource};
use serde_json::Value;
use vantage_core::Result;

/// Mock DataSource that implements SelectSource with configurable return values
#[derive(Debug, Clone)]
pub struct MockSelectSource {
    value: Value,
}

impl MockSelectSource {
    /// Create a new MockSelectSource that returns the given value
    pub fn new(value: Value) -> Self {
        Self { value }
    }
}

impl DataSource for MockSelectSource {}

impl SelectSource<serde_json::Value> for MockSelectSource {
    type Select = MockSelect;

    fn select(&self) -> Self::Select {
        MockSelect::new()
    }

    async fn execute_select(&self, _select: &Self::Select) -> Result<Vec<serde_json::Value>> {
        // Return the stored JSON value as Vec<Value>
        if let Value::Array(arr) = &self.value {
            Ok(arr.clone())
        } else {
            Ok(vec![self.value.clone()])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::selectable::Selectable;
    use serde_json::json;

    #[tokio::test]
    async fn test_mock_select_source_with_array() {
        let mock = MockSelectSource::new(json!([
            {"id": 1, "name": "Alice"},
            {"id": 2, "name": "Bob"}
        ]));

        let mut select = mock.select();
        select.set_source("users", None);
        select.add_field("name");

        let results = mock.execute_select(&select).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["name"], "Alice");
        assert_eq!(results[1]["name"], "Bob");
    }

    #[tokio::test]
    async fn test_mock_select_source_with_single_value() {
        let mock = MockSelectSource::new(json!({"count": 42}));

        let select = mock.select();
        let results = mock.execute_select(&select).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["count"], 42);
    }

    #[test]
    fn test_mock_select_creation() {
        let mock = MockSelectSource::new(json!([]));
        let select = mock.select();

        // Verify MockSelect implements Selectable
        assert!(!select.has_fields());
        assert!(!select.has_where_conditions());
        assert!(!select.has_order_by());
    }
}
