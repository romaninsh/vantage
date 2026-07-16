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
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::SEC_WEBSOCKET_PROTOCOL;
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::Message};
use tracing::{Instrument as _, warn};

use crate::SurrealConnection;
use crate::live::{Action, Notification};
use crate::{
    engine::Engine,
    error::{Result, SurrealError},
};

/// Live-query subscribers, keyed by the server's live-query id.
type LiveSubscribers = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Notification>>>>;

/// Normalise a CBOR id to a stable string key.
///
/// Request ids arrive as numeric text; live-query ids arrive as CBOR UUIDs
/// (tag 37 wrapping a 16-byte string). Both the `live` RPC result and the
/// unsolicited notification frames encode the query id the same way, so
/// normalising both through here makes them compare equal.
fn cbor_id_key(v: &CborValue) -> Option<String> {
    match v {
        CborValue::Text(t) => Some(t.clone()),
        CborValue::Integer(i) => {
            let n: i128 = (*i).into();
            Some(n.to_string())
        }
        CborValue::Tag(_, inner) => cbor_id_key(inner),
        CborValue::Bytes(b) if b.len() == 16 => Some(
            uuid::Uuid::from_slice(b)
                .map(|u| u.to_string())
                .unwrap_or_else(|_| hex::encode(b)),
        ),
        CborValue::Bytes(b) => Some(hex::encode(b)),
        _ => None,
    }
}

/// Pull a [`Notification`] out of a live-query frame's inner `result` map.
///
/// The map SurrealDB delivers is `{ action, id, record, result, session }`:
/// `action` is `CREATE`/`UPDATE`/`DELETE`, `id` is the live-query uuid, and
/// the nested `result` is the affected record. Returns `None` if it doesn't
/// look like a notification (so ordinary map-shaped responses fall through).
fn parse_notification(inner: &[(CborValue, CborValue)]) -> Option<Notification> {
    let mut action = None;
    let mut query_id = None;
    let mut record_id = None;
    let mut data = None;
    for (k, v) in inner {
        if let CborValue::Text(k) = k {
            match k.as_str() {
                "action" => {
                    if let CborValue::Text(a) = v {
                        action = Action::parse(a);
                    }
                }
                "id" => query_id = cbor_id_key(v),
                "record" => record_id = Some(v.clone()),
                "result" => data = Some(v.clone()),
                _ => {}
            }
        }
    }
    Some(Notification {
        query_id: query_id?,
        action: action?,
        record_id: record_id.unwrap_or(CborValue::Null),
        data: data.unwrap_or(CborValue::Null),
    })
}

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
    live_subscribers: LiveSubscribers,
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl WsCborEngine {
    pub async fn from_connection(connect: &SurrealConnection) -> Result<Self> {
        // rustls 0.23 needs a process-wide crypto provider before any `wss://`
        // handshake. Install ring once; a competing prior install is fine.
        static TLS_PROVIDER: std::sync::Once = std::sync::Once::new();
        TLS_PROVIDER.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });

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
            live_subscribers: Arc::new(Mutex::new(HashMap::new())),
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
        let live_subscribers = Arc::clone(&self.live_subscribers);

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
                            // Two frame shapes share this channel:
                            //   response:     {id, result|error}         (id matches a request)
                            //   notification: {id, action, result}        (id is a live-query uuid)
                            match ciborium::from_reader::<CborValue, _>(binary.as_ref()) {
                                Ok(CborValue::Map(map)) => {
                                    let mut top_id = None;
                                    let mut result = None;
                                    let mut error = None;

                                    for (key, value) in &map {
                                        if let CborValue::Text(k) = key {
                                            match k.as_str() {
                                                "id" => top_id = Some(value.clone()),
                                                "result" => result = Some(value.clone()),
                                                "error" => error = Some(value.clone()),
                                                _ => {}
                                            }
                                        }
                                    }

                                    // Live-query notifications carry no top-level id;
                                    // the live-query id and action live inside `result`:
                                    //   { result: { action, id: <uuid>, result: <record> } }
                                    if top_id.is_none()
                                        && let Some(CborValue::Map(inner)) = &result
                                        && let Some(n) = parse_notification(inner)
                                    {
                                        let tx = {
                                            let subs = live_subscribers.lock().await;
                                            subs.get(&n.query_id).cloned()
                                        };
                                        if let Some(tx) = tx {
                                            let _ = tx.send(n);
                                        }
                                        continue;
                                    }

                                    // Otherwise it is a reply — route it to the waiter.
                                    if let Some(id) = top_id.as_ref().and_then(cbor_id_key) {
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
                                Ok(_) => {}
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

    async fn register_live(
        &mut self,
        query_id: &str,
    ) -> Result<mpsc::UnboundedReceiver<Notification>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut subs = self.live_subscribers.lock().await;
        subs.insert(query_id.to_string(), tx);
        Ok(rx)
    }

    async fn unregister_live(&mut self, query_id: &str) {
        let mut subs = self.live_subscribers.lock().await;
        subs.remove(query_id);
    }
}

impl Drop for WsCborEngine {
    fn drop(&mut self) {
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
    }
}
