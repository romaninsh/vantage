//! PatternDataSource implementation
//!
//! Maps query patterns to specific responses based on exact string matching.

use crate::QuerySource;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// Mock DataSource that matches exact queries to return specific responses
#[derive(Debug, Clone)]
pub struct PatternDataSource<E = crate::Expression>
where
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
{
    patterns: HashMap<String, Value>,
    _phantom: std::marker::PhantomData<E>,
}

impl<E> PatternDataSource<E>
where
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
{
    /// Create a new PatternDataSource with no patterns
    pub fn new() -> Self {
        Self {
            patterns: HashMap::new(),
            _phantom: std::marker::PhantomData,
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
}

impl<E> Default for PatternDataSource<E>
where
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<E> QuerySource<E> for PatternDataSource<E>
where
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
{
    async fn execute(&self, expr: &E) -> Value {
        let query = format!("{:?}", expr);
        self.find_match(&query)
    }

    fn defer(
        &self,
        expr: E,
    ) -> impl Fn() -> Pin<Box<dyn Future<Output = Value> + Send>> + Send + Sync + 'static {
        let mock = self.clone();
        move || {
            let mock = mock.clone();
            let expr = expr.clone();
            Box::pin(async move {
                let query = format!("{:?}", expr);
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
    async fn test_pattern() {
        let mock = PatternDataSource::<crate::Expression>::new()
            .with_pattern("SELECT * FROM users", json!([{"name": "Alice"}]))
            .with_pattern("SELECT COUNT(*) FROM orders", json!(42));

        let user_query = expr!("SELECT * FROM users");
        let count_query = expr!("SELECT COUNT(*) FROM orders");
        assert_eq!(mock.execute(&user_query).await[0]["name"], "Alice");
        assert_eq!(mock.execute(&count_query).await, 42);
    }

    #[tokio::test]
    #[should_panic(expected = "No pattern found for query")]
    async fn test_pattern_panic_on_unknown() {
        let mock = PatternDataSource::<crate::Expression>::new()
            .with_pattern("SELECT * FROM users", json!([{"name": "Alice"}]));

        let unknown_query = expr!("SELECT * FROM unknown");
        mock.execute(&unknown_query).await;
    }
}
