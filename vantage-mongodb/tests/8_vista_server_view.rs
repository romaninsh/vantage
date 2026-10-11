//! Server-side views (`db.createView`) through the vista: a declared view is
//! read-only with no change stream, and an undeclared one fails its writes
//! and watch with an error naming the flag to set. Needs a replica set
//! (`MONGODB_URL`, e.g. `mongodb://localhost:27017/?directConnection=true`).
//! Each test runs against a fresh randomised database and drops it on the
//! way out.

#![cfg(feature = "vista")]

use std::error::Error;

use bson::doc;
use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::VantageError;
use vantage_dataset::prelude::*;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_mongodb::{AnyMongoType, MongoDB, MongoId, MongoVistaFactory};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Vista, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

const UNDECLARED: &str = "declare `mongo: { view: true }`";

fn mongo_url() -> String {
    std::env::var("MONGODB_URL")
        .unwrap_or_else(|_| "mongodb://localhost:27017/?directConnection=true".into())
}

/// A fresh database holding `tasks` (two open, one done) and `open_tasks`,
/// a server-side view of the open ones.
async fn setup() -> MongoDB {
    let db_name = format!("vantage_server_view_{}", ObjectId::new().to_hex());
    let db = MongoDB::connect(&mongo_url(), &db_name)
        .await
        .expect("connect mongo");
    db.doc_collection("tasks")
        .insert_many([
            doc! { "title": "write", "status": "open" },
            doc! { "title": "test", "status": "open" },
            doc! { "title": "ship", "status": "done" },
        ])
        .await
        .expect("seed tasks");
    db.database()
        .run_command(doc! {
            "create": "open_tasks",
            "viewOn": "tasks",
            "pipeline": [ { "$match": { "status": "open" } } ],
        })
        .await
        .expect("create view");
    db
}

async fn teardown(db: &MongoDB) {
    db.database()
        .drop()
        .await
        .unwrap_or_else(|e| eprintln!("drop test db: {e}"));
}

fn spec(mongo_block: &str) -> MongoVistaSpec {
    let yaml = format!(
        r#"
name: open_task
columns:
  _id: {{ type: object_id, flags: [id] }}
  title: {{ type: string, flags: [title] }}
  status: {{ type: string }}
mongo: {mongo_block}
"#
    );
    serde_yaml_ng::from_str(&yaml).expect("spec yaml")
}

fn open_tasks(db: &MongoDB, mongo_block: &str) -> Vista {
    MongoVistaFactory::new(db.clone())
        .with_change_streams(true)
        .build_from_spec(spec(mongo_block))
        .expect("vista")
}

fn row(title: &str) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("title".to_string(), CborValue::Text(title.into()));
    record.insert("status".to_string(), CborValue::Text("open".into()));
    record
}

fn assert_undeclared(op: &str, err: VantageError) {
    assert!(err.is_unsupported(), "{op}: wrong error kind: {err}");
    let msg = err.to_string();
    assert!(msg.contains(UNDECLARED), "{op}: {msg}");
    assert!(msg.contains("open_tasks"), "{op}: {msg}");
}

async fn stored(db: &MongoDB) -> u64 {
    db.doc_collection("tasks")
        .count_documents(doc! {})
        .await
        .expect("count tasks")
}

#[tokio::test]
async fn undeclared_view_watch_names_the_flag() -> TestResult {
    let db = setup().await;
    let vista = open_tasks(&db, "{ collection: open_tasks }");
    assert!(
        vista.can_watch(),
        "an undeclared view looks like a collection"
    );

    let Err(err) = vista.watch().await else {
        panic!("watch on a server-side view must fail");
    };
    assert_undeclared("watch", err);

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn undeclared_view_writes_name_the_flag() -> TestResult {
    let db = setup().await;
    let vista = open_tasks(&db, "{ collection: open_tasks }");
    let (id, _) = vista
        .list_values()
        .await?
        .into_iter()
        .next()
        .expect("a row");

    let new_id = ObjectId::new().to_hex();
    let err = vista.insert_value(&new_id, &row("plan")).await.unwrap_err();
    assert_undeclared("insert", err);
    let err = vista.replace_value(&id, &row("rewrite")).await.unwrap_err();
    assert_undeclared("replace", err);
    let err = vista.patch_value(&id, &row("edit")).await.unwrap_err();
    assert_undeclared("patch", err);
    let err = vista.delete(&id).await.unwrap_err();
    assert_undeclared("delete", err);
    let err = vista.delete_all().await.unwrap_err();
    assert_undeclared("delete_all", err);
    let batch = IndexMap::from([(new_id, row("plan"))]);
    let err = vista.import_values(&batch).await.unwrap_err();
    assert_undeclared("import", err);
    assert_eq!(stored(&db).await, 3);

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn declared_view_is_read_only_without_streams() -> TestResult {
    let db = setup().await;
    let vista = open_tasks(&db, "{ collection: open_tasks, view: true }");

    let caps = vista.capabilities().clone();
    for (flag, on) in [
        ("can_count", caps.can_count),
        ("can_order", caps.can_order),
        ("can_search", caps.can_search),
        ("can_fetch_page", caps.can_fetch_page),
    ] {
        assert!(on, "{flag} should be on");
    }
    for (flag, on) in [
        ("can_insert", caps.can_insert),
        ("can_update", caps.can_update),
        ("can_delete", caps.can_delete),
        ("can_import", caps.can_import),
        ("can_subscribe", caps.can_subscribe),
        ("can_confine_writes", caps.can_confine_writes),
        ("can_traverse_to_record", caps.can_traverse_to_record),
        ("can_traverse_to_set", caps.can_traverse_to_set),
        ("can_traverse_in_columns", caps.can_traverse_in_columns),
    ] {
        assert!(!on, "{flag} should be off");
    }

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(vista.get_count().await?, 2);

    let err = vista
        .insert_value(ObjectId::new().to_hex(), &row("plan"))
        .await
        .unwrap_err();
    assert!(err.is_unsupported(), "insert: {err}");
    let Err(err) = vista.watch().await else {
        panic!("watch on a declared view must be refused");
    };
    assert!(err.is_unsupported(), "watch: {err}");
    assert_eq!(stored(&db).await, 3);

    teardown(&db).await;
    Ok(())
}

/// The data-source guard refuses before reaching the server: declared over
/// a plain collection, which would accept the insert, it still fails.
#[tokio::test]
async fn declared_view_refuses_writes_before_the_server() -> TestResult {
    let db = setup().await;
    let source = db.clone().with_server_view(true);
    assert!(source.is_server_view());
    let table = Table::<MongoDB, EmptyEntity>::new("tasks", source)
        .with_id_column("_id")
        .with_column_of::<String>("title");

    let mut record = Record::new();
    record.insert("title".to_string(), AnyMongoType::new("plan".to_string()));
    let err = table
        .insert_value(MongoId::from("plan"), &record)
        .await
        .unwrap_err();
    assert!(err.is_unsupported(), "{err}");
    assert!(
        err.to_string().contains("server-side view is read-only"),
        "{err}"
    );
    assert!(table.delete_all().await.unwrap_err().is_unsupported());
    assert_eq!(stored(&db).await, 3);

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn declared_view_runs_a_pipeline_over_it() -> TestResult {
    let db = setup().await;
    let block =
        r#"{ collection: open_tasks, view: true, pipeline: [ { $match: { title: "test" } } ] }"#;
    let vista = open_tasks(&db, block);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);
    assert!(!vista.capabilities().can_insert);
    assert!(!vista.capabilities().can_subscribe);

    teardown(&db).await;
    Ok(())
}
