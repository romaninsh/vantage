//! Live-query notifications.
//!
//! SurrealDB pushes a change frame over the same WebSocket for every row a
//! `LIVE SELECT` matches. Those frames are *not* replies to a request — they
//! arrive unsolicited, tagged with the live-query id the server returned from
//! the `live` RPC. The engine's read loop demultiplexes them from ordinary
//! responses and forwards each as a [`Notification`] onto a channel that backs
//! a [`LiveStream`].

use std::pin::Pin;
use std::task::{Context, Poll};

use ciborium::Value as CborValue;
use futures::Stream;
use tokio::sync::mpsc;

/// What happened to a row a live query is watching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A row entered the watched set.
    Create,
    /// A watched row's contents changed.
    Update,
    /// A row left the watched set.
    Delete,
}

impl Action {
    /// Parse SurrealDB's `CREATE` / `UPDATE` / `DELETE` action string
    /// (case-insensitive). Returns `None` for anything else (e.g. `KILLED`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "CREATE" => Some(Action::Create),
            "UPDATE" => Some(Action::Update),
            "DELETE" => Some(Action::Delete),
            _ => None,
        }
    }
}

/// One live-query change frame: which query, what happened, and the record.
///
/// `data` is the raw CBOR record exactly as SurrealDB delivered it (a map of
/// fields including `id`). Consumers decode it with the same CBOR→value path
/// they use for ordinary reads. `record_id` is the affected row's id (a
/// SurrealDB `Thing`), present on every action — including `Delete`, where
/// `data` may be empty.
#[derive(Debug, Clone)]
pub struct Notification {
    /// The live-query id this frame belongs to (matches [`LiveStream::query_id`]).
    pub query_id: String,
    /// The kind of change.
    pub action: Action,
    /// The affected row's id, as raw CBOR (a `Thing`).
    pub record_id: CborValue,
    /// The affected record as raw CBOR (empty/`Null` on some `Delete`s).
    pub data: CborValue,
}

/// A stream of [`Notification`]s for one live query.
///
/// Yields until the underlying WebSocket closes. Dropping the stream stops
/// local delivery but does **not** send `KILL` to the server — call
/// [`SurrealClient::kill`](crate::SurrealClient::kill) with
/// [`query_id`](Self::query_id) to release the server-side query.
pub struct LiveStream {
    pub(crate) query_id: String,
    pub(crate) rx: mpsc::UnboundedReceiver<Notification>,
}

impl LiveStream {
    /// The server-assigned live-query id (a UUID string).
    pub fn query_id(&self) -> &str {
        &self.query_id
    }

    /// Await the next notification, or `None` once the connection closes.
    pub async fn recv(&mut self) -> Option<Notification> {
        self.rx.recv().await
    }
}

impl Stream for LiveStream {
    type Item = Notification;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}
