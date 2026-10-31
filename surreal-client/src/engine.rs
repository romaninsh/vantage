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
    /// Send one RPC and return its result, or the server's error.
    async fn send_message_cbor(&mut self, method: &str, params: CborValue) -> Result<CborValue>;

    /// [`send_message_cbor`](Self::send_message_cbor) inside the interactive
    /// transaction `txn`, an id the `begin` RPC returned on this connection.
    /// The default refuses: only a socket keeps a transaction open between
    /// calls.
    async fn send_message_cbor_in(
        &mut self,
        _txn: &CborValue,
        _method: &str,
        _params: CborValue,
    ) -> Result<CborValue> {
        Err(SurrealError::Protocol(
            "transactions are not supported by this engine".to_string(),
        ))
    }

    /// [`send_message_cbor`](Self::send_message_cbor) with JSON in and out.
    async fn send_message(&mut self, method: &str, params: Value) -> Result<Value> {
        let cbor_params = crate::cbor_convert::json_to_cbor(params);
        let response = self.send_message_cbor(method, cbor_params).await?;
        Ok(crate::cbor_convert::cbor_to_json(response))
    }

    /// Register interest in a live query's notifications.
    ///
    /// The caller has already issued the `live` RPC and holds the returned
    /// query id; this returns a channel the engine pushes matching
    /// [`Notification`]s onto. The default returns an error, so an engine
    /// that can't receive server-pushed frames refuses live queries.
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
