//! Live-query notifications.
//!
//! SurrealDB pushes change frames, tagged with the live-query id, over the same
//! WebSocket as replies. The engine's read loop sorts them out and forwards each
//! as a [`Notification`] to the matching [`LiveStream`].

use std::pin::Pin;
use std::task::{Context, Poll};

use ciborium::Value as CborValue;
use futures::Stream;
use tokio::sync::mpsc;

use crate::{Result, SurrealClient};

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
/// Yields until the underlying WebSocket closes. Dropping the stream sends
/// `KILL` for its query in the background, so the server-side query is
/// released without further action; the result is ignored. Outside a Tokio
/// runtime nothing is sent. Use [`close`](Self::close) to kill the query
/// deterministically and observe the outcome.
///
/// The stream holds a clone of the client that created it, which keeps the
/// connection's engine alive for as long as the stream exists.
pub struct LiveStream {
    pub(crate) query_id: String,
    pub(crate) rx: mpsc::UnboundedReceiver<Notification>,
    /// Taken once the query has been killed, so it is killed only once.
    pub(crate) client: Option<SurrealClient>,
}

impl LiveStream {
    /// Kill the server-side live query and wait for the reply.
    pub async fn close(mut self) -> Result<()> {
        match self.client.take() {
            Some(client) => client.kill(&self.query_id).await,
            None => Ok(()),
        }
    }

    /// The server-assigned live-query id (a UUID string).
    pub fn query_id(&self) -> &str {
        &self.query_id
    }

    /// Await the next notification, or `None` once the connection closes.
    pub async fn recv(&mut self) -> Option<Notification> {
        self.rx.recv().await
    }
}

impl Drop for LiveStream {
    fn drop(&mut self) {
        let Some(client) = self.client.take() else {
            return;
        };
        // `kill` locks the engine, so it must run on a task rather than block here.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let query_id = std::mem::take(&mut self.query_id);
            handle.spawn(async move {
                let _ = client.kill(&query_id).await;
            });
        }
    }
}

impl Stream for LiveStream {
    type Item = Notification;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}
