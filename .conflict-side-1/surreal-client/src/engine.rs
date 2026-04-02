use async_trait::async_trait;
use ciborium::Value as CborValue;
use serde_json::Value;

use super::error::Result;

/// Engine trait for HTTP/WebSocket abstraction
#[async_trait]
pub trait Engine: Send + Sync {
    /// Send an RPC message and get the response (JSON-based)
    async fn send_message(&mut self, method: &str, params: Value) -> Result<Value>;

    /// Send an RPC message and get the response (CBOR-based)
    async fn send_message_cbor(&mut self, method: &str, params: CborValue) -> Result<CborValue>;

    /// Check if this engine supports CBOR natively
    fn supports_cbor(&self) -> bool {
        false
    }
}
