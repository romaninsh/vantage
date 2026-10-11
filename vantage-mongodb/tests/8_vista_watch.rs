//! Live updates through the vista, over MongoDB change streams. Needs a
//! replica set (`MONGODB_URL`, e.g.
//! `mongodb://localhost:27017/?directConnection=true`). Each test runs against
//! a fresh randomised database and drops it on the way out.

#![cfg(feature = "vista")]

use std::error::Error;
use std::time::Duration;

use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use futures_util::StreamExt;
use vantage_dataset::prelude::*;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_mongodb::{MongoDB, MongoVistaFactory};
use vantage_types::Record;
use vantage_vista::{Capability, FilterOp, Vista, VistaChange, VistaChangeStream, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

const WAIT: Duration = Duration::from_secs(10);

fn mongo_url() -> String {
    std::env::var("MONGODB_URL")
        .unwrap_or_else(|_| "mongodb://localhost:27017/?directConnection=true".into())
}

async fn setup() -> MongoDB {
    let db_name = format!("vantage_vista_watch_{}", ObjectId::new().to_hex());
    MongoDB::connect(&mongo_url(), &db_name)
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
  status: { type: string }
mongo:
  collection: tasks
"#;

fn tasks(db: &MongoDB) -> Vista {
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(TASK).expect("spec yaml");
    db.vista_factory().build_from_spec(spec).expect("vista")
}

fn row(title: &str, status: &str) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("title".to_string(), CborValue::Text(title.into()));
    record.insert("status".to_string(), CborValue::Text(status.into()));
    record
}

fn status(value: &str) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("status".to_string(), CborValue::Text(value.into()));
    record
}

async fn next(changes: &mut VistaChangeStream) -> VistaChange {
    tokio::time::timeout(WAIT, changes.next())
        .await
        .expect("timed out waiting for a change")
        .expect("stream ended")
        .expect("change stream error")
}

#[tokio::test]
async fn watch_emits_insert_update_delete() -> TestResult {
    let db = setup().await;
    let vista = tasks(&db);
    assert!(vista.capabilities().has(Capability::Subscribe));

    let mut changes = vista.watch().await?;
    let id = ObjectId::new().to_hex();
    // Each change is awaited before the next write, so the re-read behind it
    // sees the state that write produced.
    vista.insert_value(&id, &row("write", "open")).await?;
    match next(&mut changes).await {
        VistaChange::Inserted { id: got, value } => {
            assert_eq!(got, id);
            assert_eq!(Some(value), vista.get_value(&id).await?);
            assert!(vista.list_values().await?.contains_key(&got));
        }
        other => panic!("expected Inserted, got {other:?}"),
    }

    vista.patch_value(&id, &status("done")).await?;
    match next(&mut changes).await {
        VistaChange::Updated { id: got, value } => {
            assert_eq!(got, id);
            assert_eq!(value.get("status"), Some(&CborValue::Text("done".into())));
            assert_eq!(Some(value), vista.get_value(&id).await?);
        }
        other => panic!("expected Updated, got {other:?}"),
    }

    vista.delete(&id).await?;
    match next(&mut changes).await {
        VistaChange::Deleted { id: got } => assert_eq!(got, id),
        other => panic!("expected Deleted, got {other:?}"),
    }

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn watch_narrowed_update_leaving_set_is_deleted() -> TestResult {
    let db = setup().await;
    let all = tasks(&db);
    let id = ObjectId::new().to_hex();
    all.insert_value(&id, &row("write", "open")).await?;

    let mut open = tasks(&db);
    open.add_condition("status", FilterOp::Eq, CborValue::Text("open".into()))?;
    let mut changes = open.watch().await?;

    all.patch_value(&id, &status("closed")).await?;

    match next(&mut changes).await {
        VistaChange::Deleted { id: got } => assert_eq!(got, id),
        other => panic!("expected Deleted, got {other:?}"),
    }

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn watch_raw_driver_write_is_seen() -> TestResult {
    let db = setup().await;
    let vista = tasks(&db);
    let mut changes = vista.watch().await?;

    let oid = ObjectId::new();
    db.doc_collection("tasks")
        .insert_one(bson::doc! { "_id": oid, "title": "raw", "status": "open" })
        .await?;

    match next(&mut changes).await {
        VistaChange::Inserted { id, value } => {
            assert_eq!(id, oid.to_hex());
            assert_eq!(Some(value), vista.get_value(&id).await?);
        }
        other => panic!("expected Inserted, got {other:?}"),
    }

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn watch_drop_ends_stream() -> TestResult {
    let db = setup().await;
    let vista = tasks(&db);
    db.doc_collection("tasks")
        .insert_one(bson::doc! { "_id": ObjectId::new(), "title": "x", "status": "open" })
        .await?;

    let mut changes = vista.watch().await?;
    db.doc_collection("tasks").drop().await?;

    let end = tokio::time::timeout(WAIT, changes.next())
        .await
        .expect("timed out waiting for the stream to end");
    assert!(end.is_none(), "expected stream end, got {end:?}");

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn capabilities_without_change_streams() -> TestResult {
    let db = setup().await;
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(TASK)?;

    let off = MongoVistaFactory::new(db.clone())
        .with_change_streams(false)
        .build_from_spec(spec.clone())?;
    assert!(!off.capabilities().has(Capability::Subscribe));
    let Err(err) = off.watch().await else {
        panic!("watch must be unsupported");
    };
    assert!(err.is_unsupported(), "wrong error kind: {err}");
    assert_eq!(
        err.context.get("capability").map(String::as_str),
        Some("\"subscribe\"")
    );

    let default = MongoVistaFactory::new(db.clone()).build_from_spec(spec)?;
    assert!(default.capabilities().has(Capability::Subscribe));

    teardown(&db).await;
    Ok(())
}
