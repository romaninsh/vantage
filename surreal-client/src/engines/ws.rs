use async_trait::async_trait;
use ciborium::Value as CborValue;
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

    async fn send_message_cbor(&mut self, _method: &str, _params: CborValue) -> Result<CborValue> {
        Err(SurrealError::Protocol(
            "CBOR not supported by regular WebSocket engine. Use ws_cbor:// scheme.".to_string(),
        ))
    }

    fn supports_cbor(&self) -> bool {
        false
    }
}
