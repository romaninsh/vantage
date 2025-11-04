use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use mobc::{Manager, Pool};
use serde_json::Value;
use std::any::Any;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::Message};
use url::Url;

use crate::surreal_client::{
    connection::AuthParams,
    engine::Engine,
    error::{Result, SurrealError},
    rpc::{RpcMessage, RpcResponse},
};

type WsStream = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub struct WsConnection {
    request_tx: mpsc::UnboundedSender<(RpcMessage, oneshot::Sender<Value>)>,
    _handle: tokio::task::JoinHandle<()>,
}

impl WsConnection {
    async fn new(
        url: String,
        namespace: Option<String>,
        database: Option<String>,
        auth: Option<AuthParams>,
    ) -> Result<Self> {
        let ws_url = format!("{}/rpc", url);
        let parsed_url = Url::parse(&ws_url)
            .map_err(|e| SurrealError::Connection(format!("Invalid URL: {}", e)))?;

        let (ws_stream, _) = connect_async(parsed_url)
            .await
            .map_err(|e| SurrealError::Connection(format!("Failed to connect: {}", e)))?;

        let (mut ws_stream) = ws_stream;

        // Initialize connection with auth and namespace directly
        Self::initialize_stream(&mut ws_stream, namespace, database, auth).await?;

        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let stream = Arc::new(Mutex::new(ws_stream));

        // Start the message handler
        let handle = tokio::spawn(Self::handle_messages(stream, request_rx));

        Ok(WsConnection {
            request_tx,
            _handle: handle,
        })
    }

    async fn initialize_stream(
        stream: &mut WsStream,
        namespace: Option<String>,
        database: Option<String>,
        auth: Option<AuthParams>,
    ) -> Result<()> {
        // Send USE command if namespace/database provided
        if namespace.is_some() || database.is_some() {
            let message = RpcMessage::new("use").with_id(1).with_params(vec![
                namespace.clone().map(Value::String).unwrap_or(Value::Null),
                database.clone().map(Value::String).unwrap_or(Value::Null),
            ]);

            Self::send_and_receive_direct(stream, message).await?;
        }

        // Send authentication if provided
        if let Some(auth_params) = auth {
            match auth_params {
                AuthParams::Root { username, password } => {
                    let message =
                        RpcMessage::new("signin")
                            .with_id(2)
                            .with_params(vec![Value::Object({
                                let mut map = serde_json::Map::new();
                                map.insert("user".to_string(), Value::String(username));
                                map.insert("pass".to_string(), Value::String(password));
                                map
                            })]);

                    Self::send_and_receive_direct(stream, message).await?;
                }
                _ => {
                    return Err(SurrealError::Protocol(
                        "Unsupported auth type for pool".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    async fn send_and_receive_direct(stream: &mut WsStream, message: RpcMessage) -> Result<Value> {
        let json_str = serde_json::to_string(&message)
            .map_err(|e| SurrealError::Serialization(e.to_string()))?;

        // Send request
        stream
            .send(Message::Text(json_str))
            .await
            .map_err(|e| SurrealError::Connection(format!("Failed to send: {}", e)))?;

        // Wait for response
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(response) = serde_json::from_str::<RpcResponse>(&text) {
                        if let Some(id) = response.id {
                            if id == message.id {
                                return Ok(response.result.unwrap_or(Value::Null));
                            }
                        }
                    }
                }
                Ok(_) => continue,
                Err(e) => return Err(SurrealError::Connection(format!("WebSocket error: {}", e))),
            }
        }

        Err(SurrealError::Connection("Connection closed".to_string()))
    }

    async fn handle_messages(
        stream: Arc<Mutex<WsStream>>,
        mut request_rx: mpsc::UnboundedReceiver<(RpcMessage, oneshot::Sender<Value>)>,
    ) {
        let mut pending_requests = std::collections::HashMap::new();

        loop {
            tokio::select! {
                // Handle outgoing requests
                request = request_rx.recv() => {
                    if let Some((message, response_tx)) = request {
                        let id = message.id;
                        pending_requests.insert(id, response_tx);

                        let json_str = match serde_json::to_string(&message) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };

                        let mut stream_guard = stream.lock().await;
                        if stream_guard.send(Message::Text(json_str)).await.is_err() {
                            break;
                        }
                    } else {
                        break; // Channel closed
                    }
                }

                // Handle incoming responses
                message = async {
                    let mut stream_guard = stream.lock().await;
                    stream_guard.next().await
                } => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(response) = serde_json::from_str::<RpcResponse>(&text) {
                                if let Some(id) = response.id {
                                    if let Some(sender) = pending_requests.remove(&id) {
                                        let _ = sender.send(response.result.unwrap_or(Value::Null));
                                    }
                                }
                            }
                        }
                        Some(Ok(_)) => continue,
                        Some(Err(_)) | None => break,
                    }
                }
            }
        }
    }

    pub async fn send_rpc(&self, message: RpcMessage) -> Result<Value> {
        let (tx, rx) = oneshot::channel();

        self.request_tx
            .send((message, tx))
            .map_err(|_| SurrealError::Connection("Connection closed".to_string()))?;

        rx.await
            .map_err(|_| SurrealError::Connection("Response channel closed".to_string()))
    }
}

pub struct WsConnectionManager {
    url: String,
    namespace: Option<String>,
    database: Option<String>,
    auth: Option<AuthParams>,
}

impl WsConnectionManager {
    pub fn new(
        url: String,
        namespace: Option<String>,
        database: Option<String>,
        auth: Option<AuthParams>,
    ) -> Self {
        Self {
            url,
            namespace,
            database,
            auth,
        }
    }
}

#[async_trait]
impl Manager for WsConnectionManager {
    type Connection = WsConnection;
    type Error = SurrealError;

    async fn connect(&self) -> std::result::Result<Self::Connection, Self::Error> {
        WsConnection::new(
            self.url.clone(),
            self.namespace.clone(),
            self.database.clone(),
            self.auth.clone(),
        )
        .await
    }

    async fn check(
        &self,
        conn: Self::Connection,
    ) -> std::result::Result<Self::Connection, Self::Error> {
        // Just return the connection - assume it's healthy
        // Real health checking would require more sophisticated logic
        Ok(conn)
    }
}

pub struct WsPoolEngine {
    pool: Pool<WsConnectionManager>,
    timeout: u64,
}

impl WsPoolEngine {
    pub fn new(url: &str, max_connections: u64) -> Self {
        Self::with_config(url, max_connections, None, None, None)
    }

    pub fn with_config(
        url: &str,
        max_connections: u64,
        namespace: Option<String>,
        database: Option<String>,
        auth: Option<AuthParams>,
    ) -> Self {
        let manager = WsConnectionManager::new(url.to_string(), namespace, database, auth);
        let pool = Pool::builder()
            .max_open(max_connections)
            .max_idle(1) // Force creation of new connections for concurrent requests
            .get_timeout(Some(Duration::from_secs(30)))
            .build(manager);

        Self { pool, timeout: 30 }
    }

    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl Engine for WsPoolEngine {
    async fn connect(&mut self) -> Result<()> {
        // Test that we can get a connection from the pool
        let conn = self.pool.get().await.map_err(|e| {
            SurrealError::Connection(format!("Failed to get connection from pool: {}", e))
        })?;

        // Return connection to pool immediately
        drop(conn);
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        // mobc doesn't have explicit close, connections will be dropped
        Ok(())
    }

    async fn rpc(&self, message: RpcMessage) -> Result<Value> {
        let conn =
            self.pool.get().await.map_err(|e| {
                SurrealError::Connection(format!("Failed to get connection: {}", e))
            })?;

        let result =
            tokio::time::timeout(Duration::from_secs(self.timeout), conn.send_rpc(message)).await;

        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(SurrealError::Timeout("RPC request timed out".to_string())),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn set_timeout(&mut self, timeout: u64) {
        self.timeout = timeout;
    }

    fn get_timeout(&self) -> u64 {
        self.timeout
    }

    async fn ping(&self) -> Result<()> {
        let ping_message = RpcMessage::new("ping").with_id(1);
        self.rpc(ping_message).await?;
        Ok(())
    }
}

impl Clone for WsPoolEngine {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            timeout: self.timeout,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ws_pool_engine_creation() {
        let engine = WsPoolEngine::new("ws://localhost:8000", 10);
        assert_eq!(engine.timeout, 30);
    }

    #[tokio::test]
    async fn test_ws_pool_engine_with_timeout() {
        let engine = WsPoolEngine::new("ws://localhost:8000", 10).with_timeout(60);
        assert_eq!(engine.timeout, 60);
    }
}
