//! Change streams run on a client of their own, observed from the server
//! through `$currentOp`. Needs a replica set (`MONGODB_URL`, e.g.
//! `mongodb://localhost:27017/?directConnection=true`). Each test runs against
//! a fresh randomised database and drops it on the way out.

#![cfg(feature = "vista")]

use std::error::Error;
use std::time::{Duration, Instant};

use bson::oid::ObjectId;
use bson::{Document, doc};
use futures_util::{StreamExt, TryStreamExt};
use mongodb::Client;
use tokio::task::JoinHandle;
use vantage_dataset::prelude::*;
use vantage_mongodb::mongodb::{DEFAULT_STREAM_POOL_SIZE, STREAM_APP_NAME};
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_mongodb::{MongoDB, MongoVistaFactory};
use vantage_vista::{Vista, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

const WAIT: Duration = Duration::from_secs(10);
const MAIN_APP_NAME: &str = "vantage-mongodb-test-main";

fn mongo_url() -> String {
    std::env::var("MONGODB_URL")
        .unwrap_or_else(|_| "mongodb://localhost:27017/?directConnection=true".into())
}

/// `MONGODB_URL` with extra connection-string options appended.
fn url_with(options: &str) -> String {
    let url = mongo_url();
    if options.is_empty() {
        return url;
    }
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}{options}")
}

fn db_name() -> String {
    format!("vantage_watch_streams_{}", ObjectId::new().to_hex())
}

async fn setup(options: &str) -> MongoDB {
    MongoDB::connect(&url_with(options), db_name())
        .await
        .expect("connect mongo")
}

async fn teardown(db: &MongoDB) {
    db.database()
        .drop()
        .await
        .unwrap_or_else(|e| eprintln!("drop test db: {e}"));
}

const TASK: &str = r#"
name: task
columns:
  _id: { type: object_id, flags: [id] }
  title: { type: string, flags: [title] }
mongo:
  collection: tasks
"#;

fn tasks(db: &MongoDB) -> Vista {
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(TASK).expect("spec yaml");
    MongoVistaFactory::new(db.clone())
        .with_change_streams(true)
        .build_from_spec(spec)
        .expect("vista")
}

/// Watch `vista` and keep its stream polled, so a `getMore` is in flight on
/// the server most of the time. Aborting the task drops the stream.
async fn poll_watch(vista: &Vista) -> JoinHandle<()> {
    let mut changes = vista.watch().await.expect("watch");
    tokio::spawn(async move { while changes.next().await.is_some() {} })
}

async fn stop(task: JoinHandle<()>) {
    task.abort();
    let _ = task.await;
}

/// Change-stream cursors on `db`'s `tasks` collection: in-flight `getMore`s
/// and idle cursors between them.
async fn stream_ops(observer: &Client, db: &MongoDB) -> Vec<Document> {
    let ns = format!("{}.tasks", db.database().name());
    let pipeline = vec![
        doc! { "$currentOp": { "allUsers": true, "idleCursors": true } },
        doc! { "$match": {
            "ns": ns,
            "$or": [ { "type": "idleCursor" }, { "command.getMore": { "$exists": true } } ],
        } },
    ];
    observer
        .database("admin")
        .aggregate(pipeline)
        .await
        .expect("$currentOp")
        .try_collect()
        .await
        .expect("$currentOp cursor")
}

fn in_flight_get_mores(ops: &[Document], app_name: &str) -> usize {
    ops.iter()
        .filter(|op| {
            op.get_document("command")
                .is_ok_and(|c| c.contains_key("getMore"))
        })
        .filter(|op| op.get_str("appName") == Ok(app_name))
        .count()
}

/// Poll `$currentOp` until `done` holds or `WAIT` passes; the last sample.
async fn wait_until(
    observer: &Client,
    db: &MongoDB,
    done: impl Fn(&[Document]) -> bool,
) -> Vec<Document> {
    let deadline = Instant::now() + WAIT;
    loop {
        let ops = stream_ops(observer, db).await;
        if done(&ops) || Instant::now() > deadline {
            return ops;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Wait for a change-stream `getMore` from `app_name`; fail if none shows.
async fn expect_get_more(observer: &Client, db: &MongoDB, app_name: &str) -> Vec<Document> {
    let ops = wait_until(observer, db, |ops| in_flight_get_mores(ops, app_name) > 0).await;
    assert!(
        in_flight_get_mores(&ops, app_name) > 0,
        "no getMore from {app_name}: {ops:?}"
    );
    ops
}

/// Watch every vista `per_vista` times, each stream polled by its own task.
async fn poll_all(vistas: &[&Vista], per_vista: usize) -> Vec<JoinHandle<()>> {
    let mut polls = Vec::new();
    for vista in vistas {
        for _ in 0..per_vista {
            polls.push(poll_watch(vista).await);
        }
    }
    polls
}

async fn stop_all(polls: Vec<JoinHandle<()>>) {
    for task in polls {
        stop(task).await;
    }
}

/// The most `getMore`s from `app_name` seen in flight at once over `span`.
async fn peak_get_mores(observer: &Client, db: &MongoDB, app_name: &str, span: Duration) -> usize {
    let deadline = Instant::now() + span;
    let mut peak = 0;
    while Instant::now() < deadline {
        peak = peak.max(in_flight_get_mores(
            &stream_ops(observer, db).await,
            app_name,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    peak
}

async fn observer() -> Client {
    Client::with_uri_str(mongo_url())
        .await
        .expect("observer client")
}

#[tokio::test]
async fn watch_runs_on_stream_client() -> TestResult {
    let db = setup(&format!("appName={MAIN_APP_NAME}")).await;
    let observer = observer().await;
    let task = poll_watch(&tasks(&db)).await;

    expect_get_more(&observer, &db, STREAM_APP_NAME).await;
    let main = peak_get_mores(&observer, &db, MAIN_APP_NAME, Duration::from_millis(1500)).await;
    assert_eq!(main, 0, "a change stream ran on the main client");

    stop(task).await;
    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn open_stream_does_not_block_reads() -> TestResult {
    // A single pooled connection: a stream sharing it would hold it for the
    // whole of every `getMore` (about a second), and reads would queue.
    let db = setup("maxPoolSize=1").await;
    let observer = observer().await;
    let vista = tasks(&db);
    let task = poll_watch(&vista).await;
    expect_get_more(&observer, &db, STREAM_APP_NAME).await;

    let mut slowest = Duration::ZERO;
    for _ in 0..8 {
        let started = Instant::now();
        vista.list_values().await?;
        slowest = slowest.max(started.elapsed());
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    assert!(
        slowest < Duration::from_millis(500),
        "a read took {slowest:?}"
    );

    stop(task).await;
    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn dropped_stream_leaves_current_op() -> TestResult {
    let db = setup("").await;
    let observer = observer().await;
    let task = poll_watch(&tasks(&db)).await;
    let ops = wait_until(&observer, &db, |ops| !ops.is_empty()).await;
    assert!(!ops.is_empty(), "change-stream cursor never appeared");

    stop(task).await;
    let ops = wait_until(&observer, &db, |ops| ops.is_empty()).await;
    assert!(ops.is_empty(), "cursor outlived its stream: {ops:?}");

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn stream_pool_size_caps_concurrent_streams() -> TestResult {
    let observer = observer().await;
    let span = Duration::from_millis(2500);

    // A zero pool would only fail on the first watch; it is raised to one.
    let narrow = setup("").await.with_stream_pool_size(0);
    assert_eq!(narrow.stream_pool_size(), 1);
    let polls = poll_all(&[&tasks(&narrow)], 3).await;
    expect_get_more(&observer, &narrow, STREAM_APP_NAME).await;
    let peak = peak_get_mores(&observer, &narrow, STREAM_APP_NAME, span).await;
    assert!(
        peak <= 1,
        "streams over a one-connection pool ran side by side (peak {peak})"
    );
    stop_all(polls).await;
    teardown(&narrow).await;

    let wide = setup("").await;
    assert_eq!(wide.stream_pool_size(), DEFAULT_STREAM_POOL_SIZE);
    let polls = poll_all(&[&tasks(&wide)], 3).await;
    let peak = peak_get_mores(&observer, &wide, STREAM_APP_NAME, span).await;
    assert!(peak >= 2, "default pool serialised streams (peak {peak})");
    stop_all(polls).await;
    teardown(&wide).await;
    Ok(())
}

#[tokio::test]
async fn clones_share_one_stream_client() -> TestResult {
    let observer = observer().await;
    let db = setup("").await.with_stream_pool_size(1);
    let (left, right) = (tasks(&db.clone()), tasks(&db.clone()));
    let polls = poll_all(&[&left, &right], 2).await;

    expect_get_more(&observer, &db, STREAM_APP_NAME).await;
    let peak = peak_get_mores(&observer, &db, STREAM_APP_NAME, Duration::from_millis(2500)).await;
    assert!(
        peak <= 1,
        "clones ran streams on separate clients (peak {peak})"
    );

    stop_all(polls).await;
    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn stream_pool_size_defaults_to_100_can_be_set_and_never_drops_below_one() -> TestResult {
    let client = Client::with_uri_str(mongo_url()).await?;
    let db = MongoDB::new(client, db_name());

    assert_eq!(DEFAULT_STREAM_POOL_SIZE, 100);
    assert_eq!(db.stream_pool_size(), 100);
    assert_eq!(db.clone().with_stream_pool_size(7).stream_pool_size(), 7);
    assert_eq!(db.with_stream_pool_size(0).stream_pool_size(), 1);
    Ok(())
}

#[tokio::test]
async fn new_handle_watches_on_its_client() -> TestResult {
    let client = Client::with_uri_str(url_with(&format!("appName={MAIN_APP_NAME}"))).await?;
    let db = MongoDB::new(client, db_name()).with_stream_pool_size(1);
    let observer = observer().await;
    let task = poll_watch(&tasks(&db)).await;

    let ops = expect_get_more(&observer, &db, MAIN_APP_NAME).await;
    assert_eq!(in_flight_get_mores(&ops, STREAM_APP_NAME), 0);

    stop(task).await;
    teardown(&db).await;
    Ok(())
}
