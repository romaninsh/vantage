use serde_json::Value;
use std::collections::HashMap;

/// Session state management for SurrealDB connections
#[derive(Debug, Clone, Default)]
pub struct SessionState {
    /// Current namespace
    pub namespace: Option<String>,

    /// Current database
    pub database: Option<String>,

    /// Authentication token
    pub token: Option<String>,

    /// Current scope (for record-level authentication)
    pub scope: Option<String>,

    /// Session parameters set via `let`
    pub params: HashMap<String, Value>,
}

impl SessionState {
    /// Create a new empty session state
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the namespace
    pub fn set_namespace(&mut self, namespace: Option<String>) {
        self.namespace = namespace;
    }

    /// Set the database
    pub fn set_database(&mut self, database: Option<String>) {
        self.database = database;
    }

    /// Set the authentication token
    pub fn set_token(&mut self, token: Option<String>) {
        self.token = token;
    }

    /// Set the scope
    pub fn set_scope(&mut self, scope: Option<String>) {
        self.scope = scope;
    }

    /// Set a session parameter
    pub fn set_param(&mut self, key: String, value: Value) {
        self.params.insert(key, value);
    }

    /// Remove a session parameter
    pub fn unset_param(&mut self, key: &str) {
        self.params.remove(key);
    }

    /// Get a session parameter
    pub fn get_param(&self, key: &str) -> Option<&Value> {
        self.params.get(key)
    }

    /// Clear all session parameters
    pub fn clear_params(&mut self) {
        self.params.clear();
    }

    /// Check if authenticated (has a token)
    pub fn is_authenticated(&self) -> bool {
        self.token.is_some()
    }

    /// Get the current namespace/database pair as a tuple
    pub fn get_target(&self) -> (Option<&String>, Option<&String>) {
        (self.namespace.as_ref(), self.database.as_ref())
    }

    /// Set both namespace and database at once
    pub fn set_target(&mut self, namespace: Option<String>, database: Option<String>) {
        self.namespace = namespace;
        self.database = database;
    }

    /// Clear all authentication data
    pub fn clear_auth(&mut self) {
        self.token = None;
        self.scope = None;
    }

    /// Reset the entire session state
    pub fn reset(&mut self) {
        self.namespace = None;
        self.database = None;
        self.token = None;
        self.scope = None;
        self.params.clear();
    }

    /// Get all parameters as a reference
    pub fn params(&self) -> &HashMap<String, Value> {
        &self.params
    }

    /// Merge parameters from another map
    pub fn merge_params(&mut self, params: HashMap<String, Value>) {
        for (key, value) in params {
            self.params.insert(key, value);
        }
    }
}
