//! Live-query integration tests against a real SurrealDB instance.
//!
//! Requires SurrealDB at localhost:8000 (root/root). Start it with
//! `vantage-surrealdb/scripts/start.sh` (docker) or
//! `surreal start --user root --pass root memory`.
//!
//! Run: `cargo test --test live_query`

use futures::StreamExt;
use serde_json::json;
use surreal_client::mocks::CallLog;
use surreal_client::{Action, SurrealConnection, SurrealMockBuilder};

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

#[tokio::test]
async fn delete_notification_carries_the_record_id() {
    let db = client("live_delete").await;
    db.query("DEFINE TABLE gadget SCHEMALESS; DELETE gadget", None)
        .await
        .expect("define + clear");
    db.query("CREATE gadget:one SET label = 'x'", None)
        .await
        .expect("create");

    let mut stream = db.live("gadget").await.expect("live");
    db.query("DELETE gadget:one", None).await.expect("delete");

    let n = tokio::time::timeout(std::time::Duration::from_secs(5), stream.recv())
        .await
        .expect("timed out waiting for a live notification")
        .expect("live stream closed unexpectedly");
    assert_eq!(n.action, Action::Delete);
    assert_eq!(n.query_id, stream.query_id());
    assert_eq!(
        n.record_id,
        ciborium::Value::Tag(
            8,
            Box::new(ciborium::Value::Array(vec![
                ciborium::Value::Text("gadget".into()),
                ciborium::Value::Text("one".into()),
            ]))
        )
    );

    stream.close().await.expect("close");
}

#[tokio::test]
async fn killed_live_query_delivers_no_further_notifications() {
    let db = client("live_kill").await;
    db.query("DEFINE TABLE gizmo SCHEMALESS; DELETE gizmo", None)
        .await
        .expect("define + clear");

    let mut stream = db.live("gizmo").await.expect("live");
    let qid = stream.query_id().to_string();
    db.kill(&qid).await.expect("kill");

    db.query("CREATE gizmo:one SET label = 'late'", None)
        .await
        .expect("create");
    let after_kill =
        tokio::time::timeout(std::time::Duration::from_millis(500), stream.recv()).await;
    assert!(
        !matches!(after_kill, Ok(Some(_))),
        "a killed live query must not deliver notifications"
    );
}

#[test]
fn action_parse_reads_change_kinds_case_insensitively() {
    assert_eq!(Action::parse("CREATE"), Some(Action::Create));
    assert_eq!(Action::parse("update"), Some(Action::Update));
    assert_eq!(Action::parse("Delete"), Some(Action::Delete));
    assert_eq!(Action::parse("KILLED"), None);
}

const MOCK_QID: &str = "11111111-2222-3333-4444-555555555555";

fn mock_client() -> (surreal_client::SurrealClient, CallLog) {
    let builder = SurrealMockBuilder::new()
        .with_exact_response("live", json!(["t"]), json!(MOCK_QID))
        .with_exact_response("kill", json!([MOCK_QID]), json!(null));
    let calls = builder.call_log();
    (builder.build(), calls)
}

fn kills(calls: &CallLog) -> usize {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter(|m| *m == "kill")
        .count()
}

async fn let_background_tasks_run() {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn live_stream_reports_the_server_query_id() {
    let (client, _) = mock_client();
    let stream = client.live("t").await.expect("live");
    assert_eq!(stream.query_id(), MOCK_QID);
    stream.close().await.expect("close");
}

#[tokio::test]
async fn dropping_a_live_stream_kills_the_query() {
    let (client, calls) = mock_client();
    let stream = client.live("t").await.expect("live");
    assert_eq!(kills(&calls), 0);

    drop(stream);
    let_background_tasks_run().await;
    assert_eq!(kills(&calls), 1);
}

#[tokio::test]
async fn closing_a_live_stream_kills_the_query_once() {
    let (client, calls) = mock_client();
    let stream = client.live("t").await.expect("live");

    stream.close().await.expect("close");
    assert_eq!(kills(&calls), 1);

    let_background_tasks_run().await;
    assert_eq!(kills(&calls), 1);
}

#[test]
fn dropping_a_live_stream_outside_a_runtime_sends_nothing() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (client, calls) = mock_client();
    let stream = rt.block_on(client.live("t")).expect("live");

    drop(stream);
    assert_eq!(kills(&calls), 0);
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
