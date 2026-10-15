//! Mock engine that answers only exact method-and-params matches.

use crate::live::Notification;
use crate::{Engine, error::Result};
use async_trait::async_trait;
use ciborium::Value as CborValue;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Shared, append-only record of the RPC method names a mock engine received.
pub type CallLog = Arc<Mutex<Vec<String>>>;

/// A mock SurrealDB engine that requires exact matching of requests
#[derive(Debug, Clone)]
pub struct MockSurrealEngine {
    exact_matches: HashMap<(String, Value), Value>,
    debug: bool,
    calls: CallLog,
    live: HashMap<String, mpsc::UnboundedSender<Notification>>,
}

impl MockSurrealEngine {
    /// Create a new mock engine
    pub fn new() -> Self {
        Self {
            exact_matches: HashMap::new(),
            debug: false,
            calls: CallLog::default(),
            live: HashMap::new(),
        }
    }

    /// Handle to the log of RPC method names this engine receives. It stays
    /// valid after the engine is moved into a client.
    pub fn call_log(&self) -> CallLog {
        self.calls.clone()
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

    /// Add a response for `method` called with params `{}`, an empty object
    pub fn with_method_response(mut self, method: impl Into<String>, response: Value) -> Self {
        self.exact_matches
            .insert((method.into(), json!({})), response);
        self
    }

    /// Add a response for a `query` call with this SQL and no variables
    pub fn with_query_response(mut self, query: impl Into<String>, response: Value) -> Self {
        let params = json!([query.into()]);
        self.exact_matches
            .insert(("query".to_string(), params), response);
        self
    }

    /// Find exact matching response for a request or panic with descriptive error
    fn find_response(&self, method: &str, params: &Value) -> Value {
        self.calls.lock().unwrap().push(method.to_string());

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

    async fn send_message_cbor(&mut self, method: &str, params: CborValue) -> Result<CborValue> {
        let json_params = crate::cbor_convert::cbor_to_json(params);
        let response = self.find_response(method, &json_params);
        Ok(crate::cbor_convert::json_to_cbor(response))
    }

    async fn register_live(
        &mut self,
        query_id: &str,
    ) -> Result<mpsc::UnboundedReceiver<Notification>> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.live.insert(query_id.to_string(), tx);
        Ok(rx)
    }

    async fn unregister_live(&mut self, query_id: &str) {
        self.live.remove(query_id);
    }
}

/// Builds a [`SurrealClient`](crate::SurrealClient) over a [`MockSurrealEngine`],
/// targeting namespace and database `test` unless set
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

    /// Handle to the log of RPC method names the built client's engine receives
    pub fn call_log(&self) -> CallLog {
        self.engine.call_log()
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

    /// Add a response for `method` called with params `{}`, an empty object
    pub fn with_method_response(mut self, method: impl Into<String>, response: Value) -> Self {
        self.engine = self.engine.with_method_response(method, response);
        self
    }

    /// Add a response for a `query` call with this SQL and no variables
    pub fn with_query_response(mut self, query: impl Into<String>, response: Value) -> Self {
        self.engine = self.engine.with_query_response(query, response);
        self
    }

    /// Build a client over the mock engine
    pub fn build(self) -> crate::SurrealClient {
        crate::SurrealClient::new(Box::new(self.engine), self.namespace, self.database)
    }
}

impl Default for SurrealMockBuilder {
    fn default() -> Self {
        Self::new()
    }
}
