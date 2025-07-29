use async_trait::async_trait;
use serde_json::Value;
use std::any::Any;

use super::{error::Result, rpc::RpcMessage};

/// Engine trait for HTTP/WebSocket abstraction
#[async_trait]
pub trait Engine: Send + Sync {
    /// Connect to the SurrealDB instance
    async fn connect(&mut self) -> Result<()>;

    /// Close the connection
    async fn close(&mut self) -> Result<()>;

    /// Send an RPC message and get the response
    async fn rpc(&self, message: RpcMessage) -> Result<Value>;

    /// Set timeout for requests in seconds
    fn set_timeout(&mut self, seconds: u64);

    /// Get current timeout in seconds
    fn get_timeout(&self) -> u64;

    /// Check if the connection is active
    async fn ping(&self) -> Result<()>;

    /// Get status information (for HTTP engines)
    async fn status(&self) -> Result<u16> {
        // Default implementation returns OK if ping succeeds
        self.ping().await?;
        Ok(200)
    }

    /// Get health information (for HTTP engines)
    async fn health(&self) -> Result<u16> {
        // Default implementation returns OK if ping succeeds
        self.ping().await?;
        Ok(200)
    }

    /// Get a reference to the underlying engine as Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Get a mutable reference to the underlying engine as Any for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
