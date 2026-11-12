//! A socket the server closes, or one that stops answering: callers get an
//! error instead of a hang, live streams end, and the next call reconnects in
//! place.
//!
//! Requires SurrealDB at localhost:8000 (root/root), bar the silent-socket
//! test, which runs its own stand-in server. The close is the one
//! SurrealDB makes when a burst of live notifications overflows a socket's
//! queue: one transaction on a second connection changes many rows that
//! several live queries on the first connection watch.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::time::{Duration, Instant};

use ciborium::Value as CborValue;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use surreal_client::{LiveStream, SurrealClient, SurrealConnection, SurrealError};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::SEC_WEBSOCKET_PROTOCOL;

const DB_URL: &str = "ws://localhost:8000";
const NAMESPACE: &str = "sc_reconnect";
const ROWS: usize = 1000;
const LIVES: usize = 5;

async fn client(database: &str) -> SurrealClient {
    SurrealConnection::new()
        .url(DB_URL)
        .namespace(NAMESPACE)
        .database(database)
        .auth_root("root", "root")
        .connect()
        .await
        .expect("connect")
}

/// A watched connection, a writer on a second one, and `LIVES` live queries
/// on the watched one over table `t`.
async fn watched(database: &str) -> (SurrealClient, SurrealClient, Vec<LiveStream>) {
    let writer = client(database).await;
    writer
        .query("REMOVE TABLE IF EXISTS t; DEFINE TABLE t SCHEMALESS", None)
        .await
        .expect("define t");
    let watched = client(database).await;
    let mut streams = Vec::new();
    for _ in 0..LIVES {
        streams.push(watched.live("t").await.expect("live"));
    }
    (watched, writer, streams)
}

/// One transaction creating `ROWS` rows: `ROWS × LIVES` notifications at
/// once, far more than SurrealDB queues per socket.
async fn overflow(writer: &SurrealClient) {
    let burst = format!("BEGIN; FOR $i IN 1..={ROWS} {{ CREATE t SET n = $i; }}; COMMIT;");
    writer.query(&burst, None).await.expect("burst");
}

/// Wait (bounded) until every stream has ended, draining what they deliver.
async fn all_end(streams: Vec<LiveStream>, within: Duration) -> bool {
    timeout(within, async {
        for mut stream in streams {
            while stream.recv().await.is_some() {}
        }
    })
    .await
    .is_ok()
}

/// Removes the test's database when dropped, passed or failed. Drop can't
/// await, and a panicking test's runtime may stop before a spawned task
/// runs, so the removal runs to completion on a thread of its own.
struct Throwaway(&'static str);

impl Drop for Throwaway {
    fn drop(&mut self) {
        let database = self.0;
        let removal = std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            runtime.block_on(async {
                let connected = SurrealConnection::new()
                    .url(DB_URL)
                    .namespace(NAMESPACE)
                    .database(database)
                    .auth_root("root", "root")
                    .connect()
                    .await;
                if let Ok(client) = connected {
                    let _ = client
                        .query(&format!("REMOVE DATABASE IF EXISTS {database}"), None)
                        .await;
                }
            });
        });
        let _ = removal.join();
    }
}

#[tokio::test]
async fn closed_socket_fails_in_flight_call() {
    let _db = Throwaway("in_flight");
    let (watched, writer, _streams) = watched("in_flight").await;
    let in_flight = {
        let watched = watched.clone();
        tokio::spawn(async move { watched.query("SLEEP 4s; RETURN 'slept'", None).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;

    overflow(&writer).await;

    let result = timeout(Duration::from_secs(2), in_flight)
        .await
        .expect("the in-flight call hung after the socket closed")
        .expect("task");
    // Usually the reply is lost with the socket; sometimes the server gets
    // its "cancelled" answer out before closing.
    let unknown_outcome =
        matches!(&result, Err(SurrealError::Connection(m)) if m.contains("may or may not"));
    let cancelled = matches!(&result, Ok(reply) if reply[0]["details"]["kind"] == "Cancelled");
    assert!(
        unknown_outcome || cancelled,
        "the in-flight call must fail, not be retried: {result:?}"
    );
}

#[tokio::test]
async fn live_stream_ends_on_close() {
    let _db = Throwaway("streams_end");
    let (_watched, writer, streams) = watched("streams_end").await;

    overflow(&writer).await;

    let ended = all_end(streams, Duration::from_secs(2)).await;
    assert!(ended, "every live stream ends when the socket closes");
}

#[tokio::test]
async fn next_call_reconnects_and_replays_use_and_let() {
    let _db = Throwaway("replay");
    let (mut watched, writer, streams) = watched("replay").await;
    watched.let_var("marker", json!("kept")).await.expect("let");
    overflow(&writer).await;
    let ended = all_end(streams, Duration::from_secs(2)).await;
    assert!(ended, "the socket never closed");

    let answer = timeout(
        Duration::from_secs(2),
        watched.query(
            "RETURN [session::ns(), session::db(), count(SELECT * FROM t), $marker]",
            None,
        ),
    )
    .await
    .expect("the next call hung")
    .expect("the next call reconnects");
    assert_eq!(
        answer[0]["result"],
        json!([NAMESPACE, "replay", ROWS, "kept"])
    );
}

/// A server that completes the WebSocket handshake and answers every request
/// but `query`, which it never answers, as a socket left half-open by a
/// sleeping laptop would. Returns its URL and how many sockets it accepted.
async fn silent_server() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("ws://{}", listener.local_addr().expect("address"));
    let accepted = Arc::new(AtomicUsize::new(0));
    let count = accepted.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            count.fetch_add(1, SeqCst);
            tokio::spawn(async move {
                // tungstenite's handshake callback fixes the error type.
                #[allow(clippy::result_large_err)]
                let cbor = |_: &Request, mut response: Response| {
                    response
                        .headers_mut()
                        .insert(SEC_WEBSOCKET_PROTOCOL, HeaderValue::from_static("cbor"));
                    Ok(response)
                };
                let Ok(mut ws) = accept_hdr_async(tcp, cbor).await else {
                    return;
                };
                while let Some(Ok(Message::Binary(frame))) = ws.next().await {
                    let Ok(CborValue::Map(request)) = ciborium::from_reader(frame.as_ref()) else {
                        continue;
                    };
                    let field = |name: &str| {
                        request
                            .iter()
                            .find(|(key, _)| key.as_text() == Some(name))
                            .map(|(_, value)| value.clone())
                    };
                    if field("method").as_ref().and_then(CborValue::as_text) == Some("query") {
                        continue;
                    }
                    let reply = CborValue::Map(vec![
                        ("id".into(), field("id").unwrap_or(CborValue::Null)),
                        ("result".into(), "surrealdb-2.0.0".into()),
                    ]);
                    let mut bytes = Vec::new();
                    ciborium::into_writer(&reply, &mut bytes).expect("encode");
                    if ws.send(Message::Binary(bytes.into())).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (url, accepted)
}

/// A socket that stops answering without closing fails the call at the
/// request deadline, with the lock released, and the next call opens a new
/// socket rather than waiting on the dead one.
#[tokio::test]
async fn a_silent_socket_fails_at_the_deadline_and_the_next_call_reconnects() {
    let (url, accepted) = silent_server().await;
    let client = SurrealConnection::new()
        .url(url)
        .auth_root("root", "root")
        .request_timeout(Duration::from_millis(200))
        .connect()
        .await
        .expect("connect");
    for attempt in 1..=2 {
        let started = Instant::now();
        let result = timeout(Duration::from_secs(2), client.query("RETURN 1", None))
            .await
            .expect("the call hung on a silent socket");
        assert!(
            matches!(&result, Err(SurrealError::Connection(m)) if m.contains("may or may not")),
            "{result:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(accepted.load(SeqCst), attempt, "one socket per call");
    }
}

#[tokio::test]
async fn live_subscribe_works_after_reconnect() {
    let _db = Throwaway("live_again");
    let (watched, writer, streams) = watched("live_again").await;
    overflow(&writer).await;
    let ended = all_end(streams, Duration::from_secs(2)).await;
    assert!(ended, "the socket never closed");

    let notification = timeout(Duration::from_secs(2), async {
        let mut stream = watched.live("t").await.expect("live after the close");
        writer
            .query("CREATE t:late SET n = 0", None)
            .await
            .expect("create");
        stream.recv().await
    })
    .await
    .expect("no notification after the reconnect")
    .expect("the new live stream ended");
    assert_eq!(notification.action, surreal_client::Action::Create);
}
