use async_trait::async_trait;
use ciborium::Value as CborValue;
use serde_json::Value;
use tokio::sync::mpsc;

use super::error::Result;
use crate::SurrealError;
use crate::live::Notification;

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

    /// Register interest in a live query's notifications.
    ///
    /// The caller has already issued the `live` RPC and holds the returned
    /// query id; this hands back the receiving end of a channel the engine's
    /// read loop pushes matching [`Notification`]s onto. Only transport engines
    /// that can receive server-pushed frames (the WebSocket engine) implement
    /// this — the default refuses, so request/response-only engines (mocks,
    /// HTTP) surface an honest error.
    async fn register_live(
        &mut self,
        _query_id: &str,
    ) -> Result<mpsc::UnboundedReceiver<Notification>> {
        Err(SurrealError::Protocol(
            "live queries are not supported by this engine".to_string(),
        ))
    }

    /// Drop local delivery for a live-query id. Does not send `KILL` — that is
    /// a separate RPC. Default is a no-op.
    async fn unregister_live(&mut self, _query_id: &str) {}
}
