use async_trait::async_trait;
use ciborium::Value as CborValue;
use serde_json::Value;

use super::error::Result;

/// Wire abstraction for SurrealDB RPC. The CBOR method is the real wire path;
/// the JSON method is a default convenience that transcodes through it.
#[async_trait]
pub trait Engine: Send + Sync {
    async fn send_message_cbor(&mut self, method: &str, params: CborValue) -> Result<CborValue>;

    async fn send_message(&mut self, method: &str, params: Value) -> Result<Value> {
        let cbor_params = crate::cbor_convert::json_to_cbor(params);
        let response = self.send_message_cbor(method, cbor_params).await?;
        Ok(crate::cbor_convert::cbor_to_json(response))
    }
}
