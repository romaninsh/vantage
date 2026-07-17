//! Live-query integration tests against a real SurrealDB instance.
//!
//! Requires SurrealDB at localhost:8000 (root/root). Start it with
//! `vantage-surrealdb/scripts/start.sh` (docker) or
//! `surreal start --user root --pass root memory`.
//!
//! Run: `cargo test --test live_query`

use futures::StreamExt;
use surreal_client::{Action, SurrealConnection};

const DB_URL: &str = "ws://localhost:8000";

async fn client(database: &str) -> surreal_client::SurrealClient {
    SurrealConnection::new()
        .url(DB_URL)
        .namespace("test")
        .database(database)
        .auth_root("root", "root")
        .connect()
        .await
        .expect("connect")
}

#[tokio::test]
async fn live_stream_reports_create_update_delete() {
    let db = client("live_crud").await;

    // The table must exist before LIVE SELECT; clean slate so a stale row
    // can't mask a bug.
    db.query("DEFINE TABLE bar_item SCHEMALESS; DELETE bar_item", None)
        .await
        .expect("define + clear");

    let mut stream = db.live("bar_item").await.expect("start live query");
    assert!(!stream.query_id().is_empty(), "live query id should be set");

    // CREATE — a fresh record enters the watched set.
    db.query(
        "CREATE bar_item:negroni SET name = 'Negroni', stock = 5",
        None,
    )
    .await
    .expect("create");

    let n = next(&mut stream).await;
    assert_eq!(n.action, Action::Create, "first frame should be a CREATE");

    // UPDATE — the same record changes.
    db.query("UPDATE bar_item:negroni SET stock = 4", None)
        .await
        .expect("update");
    let n = next(&mut stream).await;
    assert_eq!(n.action, Action::Update, "second frame should be an UPDATE");

    // DELETE — the record leaves the set.
    db.query("DELETE bar_item:negroni", None)
        .await
        .expect("delete");
    let n = next(&mut stream).await;
    assert_eq!(n.action, Action::Delete, "third frame should be a DELETE");

    // KILL — release the server-side query.
    let qid = stream.query_id().to_string();
    db.kill(&qid).await.expect("kill");
}

#[tokio::test]
async fn live_notification_carries_the_record() {
    let db = client("live_payload").await;
    db.query("DEFINE TABLE widget SCHEMALESS; DELETE widget", None)
        .await
        .expect("define + clear");

    let mut stream = db.live("widget").await.expect("live");
    db.query("CREATE widget:one SET label = 'hello', qty = 7", None)
        .await
        .expect("create");

    let n = next(&mut stream).await;
    assert_eq!(n.action, Action::Create);
    // The frame must carry the row, not just the id — decode via CBOR→JSON.
    let json = cbor_to_json(&n.data);
    assert_eq!(json.get("label").and_then(|v| v.as_str()), Some("hello"));
    assert_eq!(json.get("qty").and_then(|v| v.as_i64()), Some(7));

    let qid = stream.query_id().to_string();
    db.kill(&qid).await.expect("kill");
}

async fn next(stream: &mut surreal_client::LiveStream) -> surreal_client::Notification {
    tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("timed out waiting for a live notification")
        .expect("live stream closed unexpectedly")
}

/// CBOR→JSON for assertions (SurrealDB wraps records in tags; the plain
/// dialect unwraps them).
fn cbor_to_json(v: &ciborium::Value) -> serde_json::Value {
    vantage_types::cbor_json::cbor_to_json(&vantage_types::PlainDialect, v.clone())
}
