use async_trait::async_trait;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::SeqCst;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, oneshot};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::Message};

use crate::SurrealConnection;
use crate::{
    engine::Engine,
    error::{Result, SurrealError},
};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct WsEngine {
    // Send messages here
    sink: Arc<Mutex<SplitSink<WsStream, Message>>>,
    // Receive responses here
    stream: Arc<Mutex<SplitStream<WsStream>>>,

    msg_id: AtomicU64,
    pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
}

impl WsEngine {
    pub async fn from_connection(connect: &SurrealConnection) -> Result<Self> {
        let base_url = connect.url.as_ref().unwrap();
        let ws_url = if base_url.ends_with("/rpc") {
            base_url.clone()
        } else {
            format!("{}/rpc", base_url)
        };
        dbg!(&ws_url);

        let (stream, _) = connect_async(&ws_url).await.map_err(|e| {
            SurrealError::Connection(format!("Failed to connect to WebSocket: {}", e))
        })?;
        let (sink, stream) = stream.split();

        let mut engine = Self {
            sink: Arc::new(Mutex::new(sink)),
            stream: Arc::new(Mutex::new(stream)),
            msg_id: AtomicU64::new(0),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
        };
        engine.handle_messages();
        connect.init_ws_engine(&mut engine).await?;
        Ok(engine)
    }
    fn handle_messages(&self) {
        let stream = Arc::clone(&self.stream);
        let pending_requests = Arc::clone(&self.pending_requests);

        tokio::spawn(async move {
            loop {
                let msg = {
                    let mut stream_guard = stream.lock().await;
                    stream_guard.next().await
                };

                let msg = match msg {
                    None => {
                        println!("Stream ended");
                        break;
                    }
                    Some(Err(e)) => {
                        eprintln!("Error receiving message: {}", e);
                        break;
                    }
                    Some(Ok(msg)) => msg,
                };

                match msg {
                    Message::Text(text) => {
                        let parsed = serde_json::from_str::<Value>(&text).unwrap();
                        let id = parsed.get("id").unwrap().as_u64().unwrap();

                        let tx = {
                            let mut pending = pending_requests.lock().await;
                            pending.remove(&id)
                        };

                        if let Some(tx) = tx {
                            // Send the entire response, not just the result field
                            let _ = tx.send(parsed);
                        }
                    }
                    Message::Ping(_) => {}
                    Message::Pong(_) => {}
                    Message::Binary(bin) => {
                        println!("Received binary: {:?}", bin);
                    }
                    Message::Close(_) => {
                        println!("Connection closed");
                        break;
                    }
                    x => {
                        println!("Received something weird: {:?}", x)
                    }
                }
            }
        });
    }
}

#[async_trait]
impl Engine for WsEngine {
    async fn send_message(&mut self, method: &str, params: Value) -> Result<Value> {
        let (tx, rx) = oneshot::channel();
        let id = self.msg_id.fetch_add(1, SeqCst);

        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id, tx);
        }

        {
            let mut sink = self.sink.lock().await;
            sink.send(Message::Text(
                json!({"id": id, "method":method, "params": params})
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|e| SurrealError::Connection(format!("Failed to send message: {}", e)))?;
        }

        // Wait for the response
        let response = rx
            .await
            .map_err(|_| SurrealError::Protocol("Response channel closed".to_string()))?;

        // Extract the result field from the JSON response
        if let Some(error) = response.get("error") {
            return Err(SurrealError::Protocol(format!("Server error: {}", error)));
        }

        let result = response
            .get("result")
            .ok_or_else(|| SurrealError::Protocol("Missing result field in response".to_string()))?
            .clone();

        Ok(result)
    }

    // pub fn new(url: &str) -> Self {
    //     Self {
    //         ws_stream: Arc::new(Mutex::new(None)),
    //         pending_requests: Arc::new(Mutex::new(HashMap::new())),
    //         timeout: 30,
    //         url: url.to_string(),
    //     }
    // }

    /*
    async fn ensure_connected(&self) -> Result<()> {
        let mut stream_guard = self.ws_stream.lock().await;

        if stream_guard.is_none() {
            let url = format!("{}/rpc", self.url);
            let parsed_url = Url::parse(&url)
                .map_err(|e| SurrealError::Connection(format!("Invalid URL: {}", e)))?;

            let (ws_stream, _) = connect_async(parsed_url)
                .await
                .map_err(|e| SurrealError::Connection(format!("Failed to connect: {}", e)))?;

            *stream_guard = Some(ws_stream);
        }

        Ok(())
    }

    async fn send_request(&self, message: RpcMessage) -> Result<Value> {
        self.ensure_connected().await?;

        let (tx, rx) = oneshot::channel();
        let request_id = message.id;

        // Store the pending request
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(request_id, tx);
        }

        // Send the message
        {
            let mut stream_guard = self.ws_stream.lock().await;
            if let Some(stream) = stream_guard.as_mut() {
                let json_str = serde_json::to_string(&message)
                    .map_err(|e| SurrealError::Serialization(e.to_string()))?;

                stream.send(Message::Text(json_str)).await.map_err(|e| {
                    SurrealError::Connection(format!("Failed to send message: {}", e))
                })?;
            } else {
                return Err(SurrealError::Connection("Not connected".to_string()));
            }
        }

        // Start message handling if not already running
        self.handle_messages().await;

        // Wait for response with timeout
        let response = tokio::time::timeout(std::time::Duration::from_secs(self.timeout), rx)
            .await
            .map_err(|_| SurrealError::Timeout(format!("Request {} timed out", request_id)))?
            .map_err(|_| SurrealError::Protocol("Response channel closed".to_string()))?;

        Ok(response)
    }
    */
}

/*
#[async_trait]
impl Engine for WsEngine {
    async fn connect(&mut self) -> Result<()> {
        self.ensure_connected().await
    }

    async fn close(&mut self) -> Result<()> {
        let mut stream_guard = self.ws_stream.lock().await;
        if let Some(mut stream) = stream_guard.take() {
            let _ = stream.close(None).await;
        }
        Ok(())
    }

    async fn rpc(&self, message: RpcMessage) -> Result<Value> {
        self.send_request(message).await
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
        self.send_request(ping_message).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ws_simple_engine_creation() {
        let engine = WsEngine::new("ws://localhost:8000");
        assert_eq!(engine.url, "ws://localhost:8000");
        assert_eq!(engine.timeout, 30);
    }
}

*/
