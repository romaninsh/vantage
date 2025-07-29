use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::Message};
use url::Url;

use crate::surreal_client::{
    engine::Engine,
    error::{Result, SurrealError},
    rpc::{RpcMessage, RpcResponse},
};

type WsStream = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type PendingRequests = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

pub struct WsEngine {
    url: String,
    timeout: u64,
    pending_requests: PendingRequests,
    request_tx: Option<mpsc::UnboundedSender<(RpcMessage, oneshot::Sender<Value>)>>,
    _response_handle: Option<tokio::task::JoinHandle<()>>,
}

impl WsEngine {
    pub fn new(url: String) -> Result<Self> {
        let parsed_url = Url::parse(&url)?;

        // Ensure we have the correct protocol
        let ws_url = match parsed_url.scheme() {
            "ws" | "wss" => url,
            "http" => url.replace("http://", "ws://"),
            "https" => url.replace("https://", "wss://"),
            _ => return Err(SurrealError::Protocol("Invalid URL scheme".to_string())),
        };

        Ok(Self {
            url: ws_url,
            timeout: 30,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            request_tx: None,
            _response_handle: None,
        })
    }

    async fn handle_messages(
        mut ws_stream: WsStream,
        pending_requests: PendingRequests,
        mut request_rx: mpsc::UnboundedReceiver<(RpcMessage, oneshot::Sender<Value>)>,
    ) {
        loop {
            tokio::select! {
                // Handle incoming messages from WebSocket
                msg = ws_stream.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(response) = RpcResponse::from_json(&text) {
                                let mut pending = pending_requests.lock().await;
                                if let Some(sender) = pending.remove(&response.id) {
                                    match response.into_result() {
                                        Ok(value) => {
                                            let _ = sender.send(value);
                                        }
                                        Err(_err) => {
                                            // For now, send null on error
                                            // TODO: Better error handling
                                            let _ = sender.send(Value::Null);
                                        }
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Binary(_))) => {
                            // TODO: Handle CBOR messages
                        }
                        Some(Ok(Message::Close(_))) => {
                            break;
                        }
                        Some(Err(_)) => {
                            break;
                        }
                        None => {
                            break;
                        }
                        _ => {}
                    }
                }

                // Handle outgoing RPC requests
                request = request_rx.recv() => {
                    match request {
                        Some((rpc_message, response_sender)) => {
                            let mut pending = pending_requests.lock().await;
                            pending.insert(rpc_message.id, response_sender);

                            if let Ok(json) = rpc_message.to_json() {
                                if let Err(_) = ws_stream.send(Message::Text(json)).await {
                                    break;
                                }
                            }
                        }
                        None => break,
                    }
                }
            }
        }
    }
}

#[async_trait]
impl Engine for WsEngine {
    async fn connect(&mut self) -> Result<()> {
        let url = format!("{}/rpc", self.url);
        let (ws_stream, _) = connect_async(&url).await.map_err(|e| {
            SurrealError::Connection(format!("Failed to connect to WebSocket: {}", e))
        })?;

        let (request_tx, request_rx) = mpsc::unbounded_channel();

        let pending_requests = self.pending_requests.clone();
        let handle = tokio::spawn(Self::handle_messages(
            ws_stream,
            pending_requests,
            request_rx,
        ));

        self.request_tx = Some(request_tx);
        self._response_handle = Some(handle);

        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(tx) = self.request_tx.take() {
            drop(tx); // This will close the channel and stop the message handler
        }

        if let Some(handle) = self._response_handle.take() {
            handle.abort();
        }

        self.pending_requests.lock().await.clear();
        Ok(())
    }

    async fn rpc(&self, message: RpcMessage) -> Result<Value> {
        let request_tx = self
            .request_tx
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?;

        let (response_tx, response_rx) = oneshot::channel();

        request_tx
            .send((message, response_tx))
            .map_err(|_| SurrealError::Connection("Failed to send request".to_string()))?;

        let timeout_duration = tokio::time::Duration::from_secs(self.timeout);
        match tokio::time::timeout(timeout_duration, response_rx).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => Err(SurrealError::Connection(
                "Response channel closed".to_string(),
            )),
            Err(_) => Err(SurrealError::Timeout("Request timed out".to_string())),
        }
    }

    fn set_timeout(&mut self, seconds: u64) {
        self.timeout = seconds;
    }

    fn get_timeout(&self) -> u64 {
        self.timeout
    }

    async fn ping(&self) -> Result<()> {
        let ping_msg = RpcMessage::new("ping").with_id(999999);
        self.rpc(ping_msg).await?;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_engine_creation() {
        let engine = WsEngine::new("ws://localhost:8000".to_string());
        assert!(engine.is_ok());
    }

    #[test]
    fn test_url_scheme_conversion() {
        let engine = WsEngine::new("http://localhost:8000".to_string()).unwrap();
        assert!(engine.url.starts_with("ws://"));

        let engine = WsEngine::new("https://localhost:8000".to_string()).unwrap();
        assert!(engine.url.starts_with("wss://"));
    }
}
