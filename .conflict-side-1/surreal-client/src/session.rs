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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_session_state_creation() {
        let session = SessionState::new();
        assert!(session.namespace.is_none());
        assert!(session.database.is_none());
        assert!(session.token.is_none());
        assert!(session.params.is_empty());
    }

    #[test]
    fn test_session_state_setters() {
        let mut session = SessionState::new();

        session.set_namespace(Some("test_ns".to_string()));
        session.set_database(Some("test_db".to_string()));
        session.set_token(Some("jwt_token".to_string()));

        assert_eq!(session.namespace, Some("test_ns".to_string()));
        assert_eq!(session.database, Some("test_db".to_string()));
        assert_eq!(session.token, Some("jwt_token".to_string()));
        assert!(session.is_authenticated());
    }

    #[test]
    fn test_session_parameters() {
        let mut session = SessionState::new();

        session.set_param("user_id".to_string(), json!(123));
        session.set_param("role".to_string(), json!("admin"));

        assert_eq!(session.get_param("user_id"), Some(&json!(123)));
        assert_eq!(session.get_param("role"), Some(&json!("admin")));
        assert_eq!(session.params.len(), 2);

        session.unset_param("user_id");
        assert!(session.get_param("user_id").is_none());
        assert_eq!(session.params.len(), 1);

        session.clear_params();
        assert!(session.params.is_empty());
    }

    #[test]
    fn test_session_target() {
        let mut session = SessionState::new();

        session.set_target(Some("ns".to_string()), Some("db".to_string()));
        let (ns, db) = session.get_target();

        assert_eq!(ns, Some(&"ns".to_string()));
        assert_eq!(db, Some(&"db".to_string()));
    }

    #[test]
    fn test_session_reset() {
        let mut session = SessionState::new();

        session.set_namespace(Some("test".to_string()));
        session.set_token(Some("token".to_string()));
        session.set_param("key".to_string(), json!("value"));

        session.reset();

        assert!(session.namespace.is_none());
        assert!(session.token.is_none());
        assert!(session.params.is_empty());
        assert!(!session.is_authenticated());
    }

    #[test]
    fn test_merge_params() {
        let mut session = SessionState::new();
        session.set_param("existing".to_string(), json!("old"));

        let mut new_params = HashMap::new();
        new_params.insert("new".to_string(), json!("value"));
        new_params.insert("existing".to_string(), json!("updated"));

        session.merge_params(new_params);

        assert_eq!(session.get_param("new"), Some(&json!("value")));
        assert_eq!(session.get_param("existing"), Some(&json!("updated")));
    }
}
