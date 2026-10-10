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

#[cfg(test)]
mod tests {
    use crate::SurrealMockBuilder;
    use serde_json::json;

    const QID: &str = "11111111-2222-3333-4444-555555555555";

    fn mock() -> (crate::SurrealClient, crate::mocks::CallLog) {
        let builder = SurrealMockBuilder::new()
            .with_exact_response("live", json!(["t"]), json!(QID))
            .with_exact_response("kill", json!([QID]), json!(null));
        let calls = builder.call_log();
        (builder.build(), calls)
    }

    fn kills(calls: &crate::mocks::CallLog) -> usize {
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|m| *m == "kill")
            .count()
    }

    async fn settle() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn drop_sends_kill() {
        let (client, calls) = mock();
        let stream = client.live("t").await.unwrap();
        assert_eq!(stream.query_id(), QID);
        assert_eq!(kills(&calls), 0);

        drop(stream);
        settle().await;
        assert_eq!(kills(&calls), 1);
    }

    #[tokio::test]
    async fn close_sends_kill_once() {
        let (client, calls) = mock();
        let stream = client.live("t").await.unwrap();

        stream.close().await.unwrap();
        settle().await;
        assert_eq!(kills(&calls), 1);
    }

    #[test]
    fn drop_outside_runtime_does_not_panic() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (client, calls) = mock();
        let stream = rt.block_on(client.live("t")).unwrap();

        drop(stream);
        assert_eq!(kills(&calls), 0);
    }
}
