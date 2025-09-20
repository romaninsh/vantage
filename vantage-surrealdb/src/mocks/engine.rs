//! Mock SurrealDB Engine with Exact Request Matching
//!
//! Provides a simplified mock implementation that requires exact matching of method calls
//! and parameters, making it predictable and easy to debug.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use surreal_client::{Engine, error::Result};

/// A mock SurrealDB engine that requires exact matching of requests
#[derive(Debug, Clone)]
pub struct MockSurrealEngine {
    /// Exact method+params combinations mapped to responses
    exact_matches: HashMap<(String, Value), Value>,
    /// Enable debug logging of queries
    debug: bool,
}

impl MockSurrealEngine {
    /// Create a new mock engine
    pub fn new() -> Self {
        Self {
            exact_matches: HashMap::new(),
            debug: false,
        }
    }

    /// Enable debug logging of queries
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Add an exact response for a specific method and parameters
    pub fn with_exact_response(
        mut self,
        method: impl Into<String>,
        params: Value,
        response: Value,
    ) -> Self {
        self.exact_matches.insert((method.into(), params), response);
        self
    }

    /// Add a response for a method with empty parameters
    pub fn with_method_response(mut self, method: impl Into<String>, response: Value) -> Self {
        self.exact_matches
            .insert((method.into(), json!({})), response);
        self
    }

    /// Add a response for a query method with specific query string
    pub fn with_query_response(mut self, query: impl Into<String>, response: Value) -> Self {
        let params = json!([query.into(), {}]);
        self.exact_matches
            .insert(("query".to_string(), params), response);
        self
    }

    /// Find exact matching response for a request or panic with descriptive error
    fn find_response(&self, method: &str, params: &Value) -> Value {
        if self.debug {
            println!("MockSurrealEngine: method='{}', params={}", method, params);
        }

        let key = (method.to_string(), params.clone());

        if let Some(response) = self.exact_matches.get(&key) {
            if self.debug {
                println!(
                    "MockSurrealEngine: exact match found, returning {:?}",
                    response
                );
            }
            return response.clone();
        }

        // No exact match found - panic with descriptive error
        let allowed_patterns: Vec<String> = self
            .exact_matches
            .keys()
            .map(|(method, params)| format!("{}({})", method, params))
            .collect();

        panic!(
            "MockSurrealEngine: executed method {}({}), but allowed patterns are: {}",
            method,
            params,
            if allowed_patterns.is_empty() {
                "NONE - no patterns configured!".to_string()
            } else {
                allowed_patterns.join(", ")
            }
        );
    }
}

impl Default for MockSurrealEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Engine for MockSurrealEngine {
    async fn send_message(&mut self, method: &str, params: Value) -> Result<Value> {
        Ok(self.find_response(method, &params))
    }
}

/// Builder for creating mock SurrealDB instances with test data
pub struct SurrealMockBuilder {
    engine: MockSurrealEngine,
    namespace: Option<String>,
    database: Option<String>,
}

impl SurrealMockBuilder {
    /// Create a new mock builder
    pub fn new() -> Self {
        Self {
            engine: MockSurrealEngine::new(),
            namespace: Some("test".to_string()),
            database: Some("test".to_string()),
        }
    }

    /// Set the namespace for the mock client
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Set the database for the mock client
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// Enable debug logging
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.engine = self.engine.with_debug(debug);
        self
    }

    /// Add exact response for specific method and parameters
    pub fn with_exact_response(
        mut self,
        method: impl Into<String>,
        params: Value,
        response: Value,
    ) -> Self {
        self.engine = self.engine.with_exact_response(method, params, response);
        self
    }

    /// Add response for method with empty parameters
    pub fn with_method_response(mut self, method: impl Into<String>, response: Value) -> Self {
        self.engine = self.engine.with_method_response(method, response);
        self
    }

    /// Add response for queries with specific query string
    pub fn with_query_response(mut self, query: impl Into<String>, response: Value) -> Self {
        self.engine = self.engine.with_query_response(query, response);
        self
    }

    /// Build the SurrealDB instance with the configured mock
    pub fn build(self) -> crate::SurrealDB {
        use surreal_client::SurrealClient;

        let client = SurrealClient::new(Box::new(self.engine), self.namespace, self.database);

        crate::SurrealDB::new(client)
    }
}

impl Default for SurrealMockBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use vantage_expressions::DataSource;

    #[tokio::test]
    #[should_panic(
        expected = "executed method any_method({}), but allowed patterns are: NONE - no patterns configured!"
    )]
    async fn test_mock_engine_no_patterns_panics() {
        let mut engine = MockSurrealEngine::new();
        let _result = engine.send_message("any_method", json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn test_mock_engine_method_response() {
        let mut engine =
            MockSurrealEngine::new().with_method_response("query", json!([{"name": "John"}]));

        let result = engine.send_message("query", json!({})).await.unwrap();
        assert_eq!(result, json!([{"name": "John"}]));
    }

    #[tokio::test]
    #[should_panic(expected = "executed method select({}), but allowed patterns are: query({})")]
    async fn test_mock_engine_method_response_panics_on_unmatch() {
        let mut engine =
            MockSurrealEngine::new().with_method_response("query", json!([{"name": "John"}]));

        let _result = engine.send_message("select", json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn test_mock_engine_exact_query_response() {
        let mut engine = MockSurrealEngine::new()
            .with_query_response("SELECT * FROM users", json!([{"type": "user"}]));

        let result = engine
            .send_message("query", json!(["SELECT * FROM users", {}]))
            .await
            .unwrap();
        assert_eq!(result, json!([{"type": "user"}]));
    }

    #[tokio::test]
    async fn test_mock_engine_exact_response() {
        let mut engine = MockSurrealEngine::new().with_exact_response(
            "custom",
            json!({"param": "value"}),
            json!({"result": "success"}),
        );

        let result = engine
            .send_message("custom", json!({"param": "value"}))
            .await
            .unwrap();
        assert_eq!(result, json!({"result": "success"}));

        // This should panic because parameters don't match exactly
        let should_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                engine
                    .send_message("custom", json!({"param": "different"}))
                    .await
            })
        }));
        assert!(should_panic.is_err());
    }

    #[tokio::test]
    async fn test_surreal_mock_builder() {
        let db = SurrealMockBuilder::new()
            .with_query_response("SELECT * FROM users", json!([{"name": "Alice"}]))
            .build();

        // Test that we can execute the exact matching query
        let result = db
            .execute(&vantage_expressions::expr!("SELECT * FROM users"))
            .await;
        assert_eq!(result, json!([{"name": "Alice"}]));
    }

    #[tokio::test]
    #[should_panic(
        expected = "executed method query([\"SELECT * FROM posts\",{}]), but allowed patterns are"
    )]
    async fn test_surreal_mock_builder_panics_on_unmatch() {
        let db = SurrealMockBuilder::new()
            .with_query_response("SELECT * FROM users", json!([{"name": "Alice"}]))
            .build();

        // This should panic because we're querying "posts" but only "users" is configured
        let _result = db
            .execute(&vantage_expressions::expr!("SELECT * FROM posts"))
            .await;
    }

    #[test]
    fn test_exact_matching_only() {
        let engine = MockSurrealEngine::new()
            .with_query_response("SELECT name FROM users", json!([{"name": "Alice"}]))
            .with_query_response(
                "SELECT * FROM users",
                json!([{"name": "Alice", "email": "alice@example.com"}]),
            );

        // Test that only exact matches work
        let key1 = ("query".to_string(), json!(["SELECT name FROM users", {}]));
        let key2 = ("query".to_string(), json!(["SELECT * FROM users", {}]));
        let key3 = ("query".to_string(), json!(["SELECT name FROM posts", {}]));

        assert!(engine.exact_matches.contains_key(&key1));
        assert!(engine.exact_matches.contains_key(&key2));
        assert!(!engine.exact_matches.contains_key(&key3));
    }

    #[test]
    fn test_different_parameter_types() {
        let engine = MockSurrealEngine::new()
            .with_exact_response("method1", json!({}), json!("empty"))
            .with_exact_response("method1", json!([]), json!("array"))
            .with_exact_response("method1", json!({"key": "value"}), json!("object"));

        // All three should be different keys
        assert_eq!(engine.exact_matches.len(), 3);

        let key1 = ("method1".to_string(), json!({}));
        let key2 = ("method1".to_string(), json!([]));
        let key3 = ("method1".to_string(), json!({"key": "value"}));

        assert_eq!(engine.exact_matches.get(&key1), Some(&json!("empty")));
        assert_eq!(engine.exact_matches.get(&key2), Some(&json!("array")));
        assert_eq!(engine.exact_matches.get(&key3), Some(&json!("object")));
    }
}
