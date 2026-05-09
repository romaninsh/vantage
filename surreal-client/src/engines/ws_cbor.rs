//! WebSocket CBOR Engine for SurrealDB
//!
//! Connects with the `cbor` WebSocket subprotocol and exchanges CBOR-encoded
//! binary frames. Preserves native types (datetime, duration, recordid, bytes)
//! that JSON cannot carry. Accepts `ws://`, `wss://`, or `cbor://` URLs;
//! `cbor://` is treated as `ws://`.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, oneshot};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::SEC_WEBSOCKET_PROTOCOL;
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::Message};
use tracing::{Instrument as _, warn};

use crate::SurrealConnection;
use crate::{
    engine::Engine,
    error::{Result, SurrealError},
};

/// Request structure for CBOR WebSocket protocol
#[derive(Debug, Clone)]
struct RouterRequest {
    id: String,
    method: String,
    params: Option<CborValue>,
}

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// WebSocket engine using native CBOR for SurrealDB.
///
/// Connects with `Sec-WebSocket-Protocol: cbor` and sends/receives binary
/// CBOR frames. Authentication and `use ns/db` are performed during
/// `from_connection` so the returned engine is ready to issue queries.
pub struct WsCborEngine {
    sink: Arc<Mutex<SplitSink<WsStream, Message>>>,
    stream: Arc<Mutex<SplitStream<WsStream>>>,
    msg_id: AtomicU64,
    pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<CborValue>>>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl WsCborEngine {
    pub async fn from_connection(connect: &SurrealConnection) -> Result<Self> {
        let base_url = connect
            .url
            .as_ref()
            .ok_or_else(|| SurrealError::Connection("URL is required to connect".to_string()))?;

        let mut ws_url = if let Some(rest) = base_url.strip_prefix("cbor://") {
            format!("ws://{}", rest)
        } else {
            base_url.clone()
        };
        if !ws_url.ends_with("/rpc") {
            if ws_url.ends_with('/') {
                ws_url.push_str("rpc");
            } else {
                ws_url.push_str("/rpc");
            }
        }

        let mut request = ws_url
            .as_str()
            .into_client_request()
            .map_err(|e| SurrealError::Connection(format!("Invalid WebSocket URL: {}", e)))?;

        request
            .headers_mut()
            .insert(SEC_WEBSOCKET_PROTOCOL, HeaderValue::from_static("cbor"));

        let (stream, _response) = connect_async(request).await.map_err(|e| {
            SurrealError::Connection(format!("Failed to connect to WebSocket: {}", e))
        })?;

        let (sink, stream) = stream.split();

        let mut engine = Self {
            sink: Arc::new(Mutex::new(sink)),
            stream: Arc::new(Mutex::new(stream)),
            msg_id: AtomicU64::new(0),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            task_handle: None,
        };

        let task_handle = engine.handle_messages();
        engine.task_handle = Some(task_handle);

        connect.init_engine(&mut engine).await?;

        Ok(engine)
    }

    fn handle_messages(&self) -> tokio::task::JoinHandle<()> {
        let stream = Arc::clone(&self.stream);
        let pending_requests = Arc::clone(&self.pending_requests);

        tokio::spawn(
            async move {
                loop {
                    let msg = {
                        let mut stream_guard = stream.lock().await;
                        stream_guard.next().await
                    };

                    let msg = match msg {
                        None => break,
                        Some(Err(e)) => {
                            warn!(error = %e, "CBOR ws receive error");
                            break;
                        }
                        Some(Ok(msg)) => msg,
                    };

                    match msg {
                        Message::Text(_text) => {
                            // Ignore text messages - we only use CBOR binary
                        }
                        Message::Binary(binary) => {
                            // Parse CBOR response: {id, result} or {id, error}
                            match ciborium::from_reader(binary.as_ref()) {
                                Ok(cbor_response) => {
                                    if let CborValue::Map(map) = cbor_response {
                                        let mut id_str = None;
                                        let mut result = None;
                                        let mut error = None;

                                        for (key, value) in &map {
                                            if let CborValue::Text(k) = key {
                                                match k.as_str() {
                                                    "id" => {
                                                        if let CborValue::Text(id) = value {
                                                            id_str = Some(id.clone());
                                                        }
                                                    }
                                                    "result" => result = Some(value.clone()),
                                                    "error" => error = Some(value.clone()),
                                                    _ => {}
                                                }
                                            }
                                        }

                                        if let Some(id) = id_str {
                                            let tx = {
                                                let mut pending = pending_requests.lock().await;
                                                pending.remove(&id)
                                            };

                                            if let Some(tx) = tx {
                                                if let Some(err) = error {
                                                    let _ = tx.send(CborValue::Map(vec![(
                                                        CborValue::Text("error".to_string()),
                                                        err,
                                                    )]));
                                                } else if let Some(res) = result {
                                                    let _ = tx.send(res);
                                                } else {
                                                    let _ = tx.send(CborValue::Null);
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(error = %e, bytes = binary.len(), "CBOR parse failed");
                                }
                            }
                        }
                        Message::Ping(_) => {}
                        Message::Pong(_) => {}
                        Message::Close(_) => break,
                        _ => {}
                    }
                }
            }
            .in_current_span(),
        )
    }
}

#[async_trait]
impl Engine for WsCborEngine {
    async fn send_message_cbor(&mut self, method: &str, params: CborValue) -> Result<CborValue> {
        let (tx, rx) = oneshot::channel();
        let id = self.msg_id.fetch_add(1, SeqCst).to_string();

        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id.clone(), tx);
        }

        let request = RouterRequest {
            id: id.clone(),
            method: method.to_string(),
            params: Some(params),
        };

        let mut request_map = vec![
            (
                CborValue::Text("id".to_string()),
                CborValue::Text(request.id),
            ),
            (
                CborValue::Text("method".to_string()),
                CborValue::Text(request.method),
            ),
        ];

        if let Some(params) = request.params {
            request_map.push((CborValue::Text("params".to_string()), params));
        }

        let rpc_message = CborValue::Map(request_map);

        let mut payload = Vec::new();
        ciborium::into_writer(&rpc_message, &mut payload)
            .map_err(|e| SurrealError::Protocol(format!("CBOR encoding failed: {}", e)))?;

        {
            let mut sink = self.sink.lock().await;
            sink.send(Message::Binary(payload.into()))
                .await
                .map_err(|e| SurrealError::Connection(format!("WS send failed: {}", e)))?;
        }

        let response = rx
            .await
            .map_err(|_| SurrealError::Protocol("Response channel closed".to_string()))?;

        if let CborValue::Map(map) = &response {
            for (key, value) in map {
                if let CborValue::Text(k) = key
                    && k == "error"
                {
                    if let CborValue::Map(error_map) = value {
                        let mut code = -1;
                        let mut message = String::new();

                        for (error_key, error_value) in error_map {
                            if let CborValue::Text(error_k) = error_key {
                                match error_k.as_str() {
                                    "code" => {
                                        if let CborValue::Integer(c) = error_value {
                                            code = (*c).try_into().unwrap_or(-1);
                                        }
                                    }
                                    "message" => {
                                        if let CborValue::Text(m) = error_value {
                                            message = m.clone();
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }

                        if !message.is_empty() {
                            return Err(SurrealError::ServerError { code, message });
                        }
                    }

                    return Err(SurrealError::Protocol(format!("Server error: {:?}", value)));
                }
            }
        }

        Ok(response)
    }
}

impl Drop for WsCborEngine {
    fn drop(&mut self) {
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
    }
}
