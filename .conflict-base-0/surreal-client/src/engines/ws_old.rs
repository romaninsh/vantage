use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::Message};
use url::Url;

use crate::{
    engine::Engine,
    error::{Result, SurrealError},
    rpc::{RpcMessage, RpcResponse},
};

type WsStream = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type PendingRequests = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

pub struct WsEngine {
    url: String,
    timeout: Arc<RwLock<u64>>,
    pending_requests: PendingRequests,
    connection_state: Arc<RwLock<ConnectionState>>,
}

#[derive(Debug)]
enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Failed(String),
}

struct ConnectionState {
    request_tx: Option<mpsc::UnboundedSender<(RpcMessage, oneshot::Sender<Value>)>>,
    _response_handle: Option<tokio::task::JoinHandle<()>>,
    status: ConnectionStatus,
    reconnect_attempts: u32,
    last_error: Option<String>,
}

impl ConnectionState {
    fn new() -> Self {
        Self {
            request_tx: None,
            _response_handle: None,
            status: ConnectionStatus::Disconnected,
            reconnect_attempts: 0,
            last_error: None,
        }
    }

    fn is_connected(&self) -> bool {
        matches!(self.status, ConnectionStatus::Connected) && self.request_tx.is_some()
    }

    fn reset_attempts(&mut self) {
        self.reconnect_attempts = 0;
        self.last_error = None;
    }

    fn increment_attempts(&mut self, error: String) {
        self.reconnect_attempts += 1;
        self.last_error = Some(error);
    }
}

impl WsEngine {
    const MAX_RECONNECT_ATTEMPTS: u32 = 5;
    const RECONNECT_DELAY_MS: u64 = 1000;

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
            timeout: Arc::new(RwLock::new(30)),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            connection_state: Arc::new(RwLock::new(ConnectionState::new())),
        })
    }

    async fn handle_messages(
        mut ws_stream: WsStream,
        pending_requests: PendingRequests,
        mut request_rx: mpsc::UnboundedReceiver<(RpcMessage, oneshot::Sender<Value>)>,
        connection_state: Arc<RwLock<ConnectionState>>,
    ) {
        loop {
            tokio::select! {
                // Handle incoming messages from WebSocket
                msg = ws_stream.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            Self::handle_text_message(&text, &pending_requests).await;
                        }
                        Some(Ok(Message::Binary(data))) => {
                            Self::handle_binary_message(&data, &pending_requests).await;
                        }
                        Some(Ok(Message::Close(_))) => {
                            {
                                let mut state = connection_state.write().await;
                                state.status = ConnectionStatus::Failed("Connection closed".to_string());
                                state.increment_attempts("Connection closed".to_string());
                            }
                            Self::cleanup_pending_requests(&pending_requests, "Connection closed").await;
                            break;
                        }
                        Some(Err(e)) => {
                            let error_msg = format!("WebSocket error: {}", e);
                            eprintln!("{}", error_msg);
                            {
                                let mut state = connection_state.write().await;
                                state.status = ConnectionStatus::Failed(error_msg.clone());
                                state.increment_attempts(error_msg.clone());
                            }
                            Self::cleanup_pending_requests(&pending_requests, &error_msg).await;
                            break;
                        }
                        None => {
                            {
                                let mut state = connection_state.write().await;
                                state.status = ConnectionStatus::Failed("Connection closed unexpectedly".to_string());
                                state.increment_attempts("Connection closed unexpectedly".to_string());
                            }
                            Self::cleanup_pending_requests(&pending_requests, "Connection closed unexpectedly").await;
                            break;
                        }
                        _ => {}
                    }
                }

                // Handle outgoing RPC requests
                request = request_rx.recv() => {
                    match request {
                        Some((rpc_message, response_sender)) => {
                            if let Err(e) = Self::send_request(&mut ws_stream, rpc_message, response_sender, &pending_requests).await {
                                eprintln!("Failed to send request: {}", e);
                                // Mark connection as failed
                                {
                                    let mut state = connection_state.write().await;
                                    state.status = ConnectionStatus::Failed(e.to_string());
                                    state.increment_attempts(e.to_string());
                                }
                                break;
                            }
                        }
                        None => break,
                    }
                }

            }
        }

        // Mark connection as disconnected when handler exits
        {
            let mut state = connection_state.write().await;
            state.status = ConnectionStatus::Failed("Message handler stopped".to_string());
            state.increment_attempts("Message handler stopped".to_string());
        }
    }

    async fn handle_text_message(text: &str, pending_requests: &PendingRequests) {
        // Try to parse as single response first
        if let Ok(response) = RpcResponse::from_json(text) {
            Self::handle_single_response(response, pending_requests).await;
        } else if let Ok(responses) = serde_json::from_str::<Vec<RpcResponse>>(text) {
            // Handle batch responses
            for response in responses {
                Self::handle_single_response(response, pending_requests).await;
            }
        }
    }

    async fn handle_binary_message(data: &[u8], _pending_requests: &PendingRequests) {
        // TODO: Handle CBOR messages
        let _ = data; // Suppress unused variable warning
    }

    async fn handle_single_response(response: RpcResponse, pending_requests: &PendingRequests) {
        let mut pending = pending_requests.lock().await;
        if let Some(id) = response.id {
            if let Some(sender) = pending.remove(&id) {
                match response.into_result() {
                    Ok(value) => {
                        let _ = sender.send(value);
                    }
                    Err(err) => {
                        let _ = sender.send(json!({
                            "error": err.to_string()
                        }));
                    }
                }
            }
        }
    }

    async fn send_request(
        ws_stream: &mut WsStream,
        rpc_message: RpcMessage,
        response_sender: oneshot::Sender<Value>,
        pending_requests: &PendingRequests,
    ) -> Result<()> {
        // Add to pending requests
        {
            let mut pending = pending_requests.lock().await;
            pending.insert(rpc_message.id, response_sender);
        }

        // Send the message
        if let Ok(json) = rpc_message.to_json() {
            if let Err(e) = ws_stream.send(Message::Text(json.into())).await {
                // Remove from pending and return error
                let mut pending = pending_requests.lock().await;
                if let Some(sender) = pending.remove(&rpc_message.id) {
                    let _ = sender.send(json!({
                        "error": format!("Failed to send message: {}", e)
                    }));
                }
                return Err(SurrealError::Connection(format!(
                    "Failed to send message: {}",
                    e
                )));
            }
        } else {
            // Remove from pending
            let mut pending = pending_requests.lock().await;
            if let Some(sender) = pending.remove(&rpc_message.id) {
                let _ = sender.send(json!({
                    "error": "Failed to serialize message"
                }));
            }
            return Err(SurrealError::Connection(
                "Failed to serialize message".to_string(),
            ));
        }

        Ok(())
    }

    async fn cleanup_pending_requests(pending_requests: &PendingRequests, error_msg: &str) {
        let mut pending = pending_requests.lock().await;
        for (_, sender) in pending.drain() {
            let _ = sender.send(json!({
                "error": error_msg
            }));
        }
    }

    async fn ensure_connected(&self) -> Result<()> {
        // Check if already connected (read lock first for performance)
        {
            let state = self.connection_state.read().await;
            if state.is_connected() {
                return Ok(());
            }
        }

        self.connect_with_retry().await
    }

    async fn connect_with_retry(&self) -> Result<()> {
        // Need to connect - acquire write lock
        let mut state = self.connection_state.write().await;

        // Double-check after acquiring write lock (another task might have connected)
        if state.is_connected() {
            return Ok(());
        }

        // Check if we've exceeded retry attempts
        if state.reconnect_attempts >= Self::MAX_RECONNECT_ATTEMPTS {
            let error = format!(
                "Max reconnection attempts ({}) exceeded. Last error: {}",
                Self::MAX_RECONNECT_ATTEMPTS,
                state.last_error.as_deref().unwrap_or("Unknown error")
            );
            state.status = ConnectionStatus::Failed(error.clone());
            return Err(SurrealError::Connection(error));
        }

        // Add delay between reconnection attempts
        if state.reconnect_attempts > 0 {
            let delay = Self::RECONNECT_DELAY_MS * (1 << state.reconnect_attempts.min(5)) as u64;
            drop(state); // Release lock during delay
            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            state = self.connection_state.write().await;
        }

        state.status = ConnectionStatus::Connecting;

        // Close existing connection if any
        if let Some(tx) = state.request_tx.take() {
            drop(tx);
        }
        if let Some(handle) = state._response_handle.take() {
            handle.abort();
        }

        // Establish new connection
        let url = format!("{}/rpc", self.url);
        let connect_result = connect_async(&url).await;

        match connect_result {
            Ok((ws_stream, _)) => {
                let (request_tx, request_rx) = mpsc::unbounded_channel();

                let pending_requests = self.pending_requests.clone();
                let connection_state = self.connection_state.clone();
                let handle = tokio::spawn(Self::handle_messages(
                    ws_stream,
                    pending_requests,
                    request_rx,
                    connection_state,
                ));

                state.request_tx = Some(request_tx);
                state._response_handle = Some(handle);
                state.status = ConnectionStatus::Connected;
                state.reset_attempts();

                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Failed to connect to WebSocket: {}", e);
                state.increment_attempts(error_msg.clone());
                state.status = ConnectionStatus::Failed(error_msg.clone());
                Err(SurrealError::Connection(error_msg))
            }
        }
    }

    pub async fn is_connected(&self) -> bool {
        let state = self.connection_state.read().await;
        state.is_connected()
    }

    pub async fn connection_status(&self) -> String {
        let state = self.connection_state.read().await;
        match &state.status {
            ConnectionStatus::Disconnected => "Disconnected".to_string(),
            ConnectionStatus::Connecting => "Connecting".to_string(),
            ConnectionStatus::Connected => "Connected".to_string(),
            ConnectionStatus::Failed(error) => {
                format!("Failed: {} (attempts: {})", error, state.reconnect_attempts)
            }
        }
    }

    pub async fn force_reconnect(&self) -> Result<()> {
        // Force close existing connection
        {
            let mut state = self.connection_state.write().await;

            if let Some(tx) = state.request_tx.take() {
                drop(tx);
            }

            if let Some(handle) = state._response_handle.take() {
                handle.abort();
            }

            state.status = ConnectionStatus::Disconnected;
            state.reset_attempts();
        }

        // Clean up pending requests
        Self::cleanup_pending_requests(&self.pending_requests, "Force reconnecting").await;

        // Establish new connection
        self.connect_with_retry().await
    }
}

impl Clone for WsEngine {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            timeout: self.timeout.clone(),
            pending_requests: self.pending_requests.clone(),
            connection_state: self.connection_state.clone(),
        }
    }
}

#[async_trait]
impl Engine for WsEngine {
    async fn connect(&mut self) -> Result<()> {
        self.ensure_connected().await
    }

    async fn close(&mut self) -> Result<()> {
        let mut state = self.connection_state.write().await;

        if let Some(tx) = state.request_tx.take() {
            drop(tx); // This will close the channel and stop the message handler
        }

        if let Some(handle) = state._response_handle.take() {
            handle.abort();
        }

        state.status = ConnectionStatus::Disconnected;
        state.reset_attempts();

        // Clean up pending requests
        Self::cleanup_pending_requests(&self.pending_requests, "Connection closed").await;

        Ok(())
    }

    async fn rpc(&self, message: RpcMessage) -> Result<Value> {
        // Ensure we're connected (lazy connection)
        self.ensure_connected().await?;

        let state = self.connection_state.read().await;
        let request_tx = state
            .request_tx
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("Not connected".to_string()))?
            .clone();
        drop(state); // Release read lock

        let (response_tx, response_rx) = oneshot::channel();

        request_tx.send((message, response_tx)).map_err(|_| {
            SurrealError::Connection("Failed to send request - channel closed".to_string())
        })?;

        let timeout_duration = tokio::time::Duration::from_secs(*self.timeout.read().await);
        match tokio::time::timeout(timeout_duration, response_rx).await {
            Ok(Ok(value)) => {
                // Check if it's an error response
                if let Some(error) = value.get("error").and_then(|e| e.as_str()) {
                    Err(SurrealError::Rpc(error.to_string()))
                } else {
                    Ok(value)
                }
            }
            Ok(Err(_)) => Err(SurrealError::Connection(
                "Response channel closed".to_string(),
            )),
            Err(_) => Err(SurrealError::Timeout("Request timed out".to_string())),
        }
    }

    fn set_timeout(&mut self, seconds: u64) {
        // Use blocking write since this is a sync method
        if let Ok(mut timeout) = self.timeout.try_write() {
            *timeout = seconds;
        }
    }

    fn get_timeout(&self) -> u64 {
        // Use blocking read since this is a sync method
        self.timeout.try_read().map(|t| *t).unwrap_or(30)
    }

    async fn ping(&self) -> Result<()> {
        let ping_msg = RpcMessage::new("ping").with_id(999999);
        self.rpc(ping_msg).await?;
        Ok(())
    }

    async fn status(&self) -> Result<u16> {
        let state = self.connection_state.read().await;
        match &state.status {
            ConnectionStatus::Connected => Ok(200),
            ConnectionStatus::Connecting => Ok(102), // Processing
            ConnectionStatus::Disconnected => Ok(503), // Service Unavailable
            ConnectionStatus::Failed(_) => Ok(500),  // Internal Server Error
        }
    }

    async fn health(&self) -> Result<u16> {
        match self.ping().await {
            Ok(_) => Ok(200),
            Err(_) => Ok(500),
        }
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

    #[test]
    fn test_ws_engine_clone() {
        let engine = WsEngine::new("ws://localhost:8000".to_string()).unwrap();
        let cloned = engine.clone();

        // Both should have the same URL
        assert_eq!(engine.url, cloned.url);
    }

    #[tokio::test]
    async fn test_connection_state() {
        let engine = WsEngine::new("ws://localhost:8000".to_string()).unwrap();

        // Initially not connected
        assert!(!engine.is_connected().await);

        // Status should reflect disconnected state
        let status = engine.status().await.unwrap();
        assert_eq!(status, 503);

        // Check connection status
        let status_str = engine.connection_status().await;
        assert_eq!(status_str, "Disconnected");
    }

    #[tokio::test]
    async fn test_reconnection_limits() {
        let engine = WsEngine::new("ws://invalid-host:8000".to_string()).unwrap();

        // Attempt to connect should fail after max attempts
        let result = engine.ensure_connected().await;
        assert!(result.is_err());

        let status_str = engine.connection_status().await;
        assert!(status_str.starts_with("Failed:"));
    }

    #[tokio::test]
    async fn test_concurrent_requests() {
        // This test would require a running SurrealDB instance
        // Demonstrates that multiple concurrent requests can be handled

        let engine = WsEngine::new("ws://localhost:8000".to_string()).unwrap();

        // Simulate multiple concurrent requests
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let eng = engine.clone();
                tokio::spawn(async move {
                    let msg = RpcMessage::new("ping").with_id(i as u64);
                    eng.rpc(msg).await
                })
            })
            .collect();

        // Wait for all requests to complete
        for handle in handles {
            let _ = handle.await;
        }
    }
}
