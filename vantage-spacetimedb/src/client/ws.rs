//! The subscription half of the protocol: one WebSocket, many query sets.
//!
//! # Why one socket
//!
//! SpacetimeDB subscriptions are **per connection**, keyed by a client-chosen
//! `QuerySetId`. So a page showing eight tables needs one socket with eight
//! query sets, not eight sockets — which is why [`SpacetimeDb`] holds the
//! connection and hands out receivers, rather than each shell dialling out.
//!
//! # Protocol
//!
//! Subprotocol `v2.bsatn.spacetimedb`. Every server frame begins with a
//! **compression tag byte**, and the host defaults to Brotli — so the URL asks
//! for `?compression=None` explicitly. Accepting the default would mean carrying
//! a decompressor to read a demo-sized change feed.
//!
//! Rows arrive as BSATN inside a `BsatnRowList`, decoded against the table's
//! `ProductType` from the module schema. Unlike the JSON protocol this loses no
//! integer precision, which matters when a row's identity is a `u64`.
//!
//! # Updates are not an event
//!
//! A `PersistentTableRows` carries `inserts` and `deletes`. A row *changing*
//! arrives as a delete plus an insert of the same key inside one transaction —
//! there is no update variant. Pairing them by id is what turns the wire format
//! back into [`VistaChange::Updated`], and getting it wrong shows up as rows
//! flickering out of and back into a grid.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::{SinkExt, StreamExt};
use spacetimedb_client_api_messages::websocket::common::SERVER_MSG_COMPRESSION_TAG_NONE;
use spacetimedb_client_api_messages::websocket::v2::{
    ClientMessage, QuerySetId, ServerMessage, Subscribe, TableUpdateRows,
};
use spacetimedb_sats::ProductType;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, SEC_WEBSOCKET_PROTOCOL};
use tokio_tungstenite::tungstenite::protocol::Message;
use vantage_core::{Result, error};

use super::Inner;

/// One table's worth of change from a single transaction, still undecoded.
#[derive(Debug, Default)]
pub struct TableDelta {
    pub inserts: Vec<Vec<u8>>,
    pub deletes: Vec<Vec<u8>>,
    /// `true` when the host sent these as *event*-table rows.
    ///
    /// Worth surfacing separately: an event table's rows never persist, so a
    /// listing of that table is always empty however many of these arrive. This
    /// is also the only way to identify one on a host that only serves schema
    /// v9, where `is_event` does not exist.
    pub ephemeral: bool,
}

/// A live subscription. Dropping it ends the stream.
pub struct Subscription {
    pub(crate) rx: mpsc::UnboundedReceiver<Result<TableDelta>>,
}

impl Subscription {
    /// The next batch of changes, or `None` when the connection has ended.
    ///
    /// Stream end is normal, not an error — connections drop and sessions
    /// expire. Callers resubscribe; `Dio::watch` already does.
    pub async fn recv(&mut self) -> Option<Result<TableDelta>> {
        self.rx.recv().await
    }
}

/// Query-set ids are per connection, so a simple counter suffices.
static NEXT_QUERY_SET: AtomicU64 = AtomicU64::new(1);

/// Open a socket for one subscription query.
///
/// A connection per subscription is the simple form; the shared-socket
/// multiplexer is the next step, and this function is the piece it will reuse.
pub(crate) async fn subscribe(
    inner: &Arc<Inner>,
    query: String,
    row_type: ProductType,
    table: String,
) -> Result<Subscription> {
    let ws_url = websocket_url(&inner.base_url, &inner.database);
    let mut request = ws_url.as_str().into_client_request().map_err(|e| {
        error!(
            "invalid SpacetimeDB WebSocket URL",
            url = ws_url.clone(),
            error = e.to_string()
        )
    })?;

    request.headers_mut().insert(
        SEC_WEBSOCKET_PROTOCOL,
        spacetimedb_client_api_messages::websocket::v2::BIN_PROTOCOL
            .parse()
            .expect("static protocol name is a valid header value"),
    );
    if let Some(token) = &inner.token {
        request.headers_mut().insert(
            AUTHORIZATION,
            format!("Bearer {token}")
                .parse()
                .map_err(|_| error!("token is not a valid HTTP header value"))?,
        );
    }

    let (mut socket, _response) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| {
            error!(
                "could not open a SpacetimeDB subscription socket",
                url = ws_url.clone(),
                error = e.to_string()
            )
        })?;

    let query_set_id = QuerySetId::new(NEXT_QUERY_SET.fetch_add(1, Ordering::Relaxed) as u32);
    let subscribe = ClientMessage::Subscribe(Subscribe {
        request_id: 1,
        query_set_id,
        query_strings: Box::new([query.clone().into_boxed_str()]),
    });
    let payload = spacetimedb_sats::bsatn::to_vec(&subscribe)
        .map_err(|e| error!("could not encode Subscribe", error = e.to_string()))?;
    socket
        .send(Message::Binary(payload))
        .await
        .map_err(|e| error!("could not send Subscribe", error = e.to_string()))?;

    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        while let Some(frame) = socket.next().await {
            let bytes = match frame {
                Ok(Message::Binary(b)) => b,
                // Text frames are not part of the binary subprotocol; ping/pong
                // and close are handled by tungstenite or end the loop.
                Ok(Message::Close(_)) => break,
                Ok(_) => continue,
                Err(e) => {
                    let _ = tx.send(Err(error!(
                        "SpacetimeDB subscription socket failed",
                        error = e.to_string()
                    )));
                    break;
                }
            };

            match decode_server_message(&bytes) {
                Ok(Some(message)) => {
                    for delta in deltas_for_table(message, &table, &row_type) {
                        if tx.send(delta).is_err() {
                            return; // receiver dropped — the subscription is over
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    if tx.send(Err(e)).is_err() {
                        return;
                    }
                }
            }
        }
        // Falling out of the loop closes the channel, which the consumer reads
        // as a normal stream end.
    });

    Ok(Subscription { rx })
}

/// `http(s)://host/…` → `ws(s)://host/v1/database/:db/subscribe?compression=None`.
fn websocket_url(base_url: &str, database: &str) -> String {
    let ws_base = if let Some(rest) = base_url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base_url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base_url.to_string()
    };
    // `compression=None` is deliberate: the host defaults to Brotli, and reading
    // a compressed feed would mean carrying a decompressor for no benefit here.
    format!("{ws_base}/v1/database/{database}/subscribe?compression=None")
}

/// Strip the compression tag and decode a `ServerMessage`.
fn decode_server_message(bytes: &[u8]) -> Result<Option<ServerMessage>> {
    let Some((&tag, body)) = bytes.split_first() else {
        return Ok(None);
    };
    if tag != SERVER_MSG_COMPRESSION_TAG_NONE {
        return Err(error!(
            "host sent a compressed frame despite `compression=None`; this driver \
             carries no decompressor",
            tag = tag as i64
        )
        .mark_unsupported());
    }
    let message: ServerMessage = spacetimedb_sats::bsatn::from_slice(body)
        .map_err(|e| error!("could not decode a ServerMessage", error = e.to_string()))?;
    Ok(Some(message))
}

/// Extract the deltas that concern one table.
///
/// Yields `Result`s rather than bare deltas so a `SubscriptionError` can travel
/// the same path as the rows it would have carried. Dropping it instead leaves
/// the consumer holding a stream that is open, silent, and no longer subscribed
/// to anything — with nothing to trigger the reconnect that would repair it.
fn deltas_for_table(
    message: ServerMessage,
    table: &str,
    _row_type: &ProductType,
) -> Vec<Result<TableDelta>> {
    let mut out = Vec::new();
    match message {
        // The initial state of the subscription: every matching row, as inserts.
        ServerMessage::SubscribeApplied(applied) => {
            for single in applied.rows.tables.iter() {
                if single.table.as_ref() != table {
                    continue;
                }
                out.push(Ok(TableDelta {
                    inserts: (&single.rows).into_iter().map(|b| b.to_vec()).collect(),
                    deletes: Vec::new(),
                    ephemeral: false,
                }));
            }
        }
        ServerMessage::TransactionUpdate(update) => {
            for query_set in update.query_sets.iter() {
                for table_update in query_set.tables.iter() {
                    if table_update.table_name.as_ref() != table {
                        continue;
                    }
                    for rows in table_update.rows.iter() {
                        out.push(Ok(match rows {
                            TableUpdateRows::PersistentTable(p) => TableDelta {
                                inserts: (&p.inserts).into_iter().map(|b| b.to_vec()).collect(),
                                deletes: (&p.deletes).into_iter().map(|b| b.to_vec()).collect(),
                                ephemeral: false,
                            },
                            TableUpdateRows::EventTable(e) => TableDelta {
                                inserts: (&e.events).into_iter().map(|b| b.to_vec()).collect(),
                                deletes: Vec::new(),
                                ephemeral: true,
                            },
                        }));
                    }
                }
            }
        }
        // A subscription that failed after being applied. The rows we have are
        // now unmaintained, so this has to reach the consumer: `Dio::watch()`
        // treats a stream error as its cue to back off, resubscribe and
        // reconcile, and that is the only thing that gets the set correct again.
        ServerMessage::SubscriptionError(err) => {
            out.push(Err(error!(
                "SpacetimeDB ended the subscription",
                error = err.error.to_string()
            )));
        }
        _ => {}
    }
    out
}

/// Decode one BSATN row against the table's row type.
pub(crate) fn decode_row(
    bytes: &[u8],
    row_type: &ProductType,
) -> Result<spacetimedb_sats::ProductValue> {
    let mut reader = bytes;
    spacetimedb_sats::bsatn::decode(row_type, &mut reader)
        .map_err(|e| error!("could not decode a pushed row", error = e.to_string()))
}

/// Unused for now; kept so the multiplexer has a home.
#[allow(dead_code)]
type QuerySets = HashMap<u32, String>;
