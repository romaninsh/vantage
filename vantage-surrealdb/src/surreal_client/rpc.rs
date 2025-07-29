use serde::{Deserialize, Serialize};
use serde_json::Value;

/// RPC message structure for SurrealDB communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcMessage {
    pub id: u64,
    pub method: String,
    pub params: Vec<Value>,
}

impl RpcMessage {
    /// Create a new RPC message with the given method
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            id: 0,
            method: method.into(),
            params: Vec::new(),
        }
    }

    /// Set the ID for this message
    pub fn with_id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    /// Set the parameters for this message
    pub fn with_params(mut self, params: Vec<Value>) -> Self {
        self.params = params;
        self
    }

    /// Add a single parameter to this message
    pub fn with_param(mut self, param: Value) -> Self {
        self.params.push(param);
        self
    }

    /// Convert to JSON string for HTTP transport
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    /// Create from JSON string
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}

/// RPC response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// RPC error structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

impl RpcResponse {
    /// Create from JSON string
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    /// Check if this response contains an error
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Get the result or return an error
    pub fn into_result(self) -> Result<Value, crate::surreal_client::error::SurrealError> {
        match self.error {
            Some(err) => Err(crate::surreal_client::error::SurrealError::Rpc(err.message)),
            None => Ok(self.result.unwrap_or(Value::Null)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_message_creation() {
        let msg = RpcMessage::new("ping")
            .with_id(1)
            .with_param(Value::String("test".to_string()));

        assert_eq!(msg.id, 1);
        assert_eq!(msg.method, "ping");
        assert_eq!(msg.params.len(), 1);
    }

    #[test]
    fn test_rpc_message_serialization() {
        let msg = RpcMessage::new("query").with_id(42).with_params(vec![
            Value::String("SELECT * FROM user".to_string()),
            Value::Object(serde_json::Map::new()),
        ]);

        let json = msg.to_json().unwrap();
        let deserialized = RpcMessage::from_json(&json).unwrap();

        assert_eq!(msg.id, deserialized.id);
        assert_eq!(msg.method, deserialized.method);
        assert_eq!(msg.params.len(), deserialized.params.len());
    }
}
