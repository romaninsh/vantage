use async_trait::async_trait;
use serde_json::Value;
use std::any::Any;

use super::{error::Result, rpc::RpcMessage};

/// Engine trait for HTTP/WebSocket abstraction
#[async_trait]
pub trait Engine: Send + Sync {
    /// Send an RPC message and get the response
    // async fn rpc(&self, message: RpcMessage) -> Result<Value>;
    async fn send_message(&mut self, method: &str, params: Value) -> Result<Value>;
}
