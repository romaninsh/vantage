//! Transactional bulk import through the vista. Needs a replica set
//! (`MONGODB_URL`, e.g. `mongodb://localhost:27017/?directConnection=true`).
//! Each test runs against a fresh randomised database and drops it on the way
//! out.

#![cfg(feature = "vista")]

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bson::oid::ObjectId;
use bson::{Document, doc};
use ciborium::Value as CborValue;
use futures_util::TryStreamExt;
use indexmap::IndexMap;
use mongodb::IndexModel;
use mongodb::options::IndexOptions;
use vantage_dataset::prelude::*;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_mongodb::{AnyMongoType, MongoDB, MongoVistaFactory};
use vantage_table::table::{AfterFn, BeforeFn, Hook, Phase, Table};
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Vista, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL")
        .unwrap_or_else(|_| "mongodb://localhost:27017/?directConnection=true".into())
}

async fn setup() -> MongoDB {
    let db_name = format!("vantage_vista_import_{}", ObjectId::new().to_hex());
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
  sku: { type: string }
  title: { type: string, flags: [title] }
  status: { type: string }
mongo:
  collection: tasks
"#;

const CLIENT: &str = r#"
name: client
columns:
  _id: { type: object_id, flags: [id] }
  name: { type: string, flags: [title] }
  city:
    type: string
    mongo:
      nested_path: address.city
mongo:
  collection: clients
"#;

fn vista(db: &MongoDB, yaml: &str) -> Vista {
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(yaml).expect("spec yaml");
    db.vista_factory().build_from_spec(spec).expect("vista")
}

fn row(title: &str, status: &str) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("title".to_string(), CborValue::Text(title.into()));
    record.insert("status".to_string(), CborValue::Text(status.into()));
    record
}

fn sku_row(sku: &str) -> Record<CborValue> {
    let mut record = row("t", "open");
    record.insert("sku".to_string(), CborValue::Text(sku.into()));
    record
}

fn batch(rows: Vec<(ObjectId, Record<CborValue>)>) -> IndexMap<String, Record<CborValue>> {
    rows.into_iter().map(|(id, r)| (id.to_hex(), r)).collect()
}

async fn stored(
    db: &MongoDB,
    collection: &str,
) -> std::result::Result<Vec<Document>, Box<dyn Error>> {
    let mut docs: Vec<Document> = db
        .doc_collection(collection)
        .find(doc! {})
        .await?
        .try_collect()
        .await?;
    docs.sort_by_key(|d| {
        d.get_object_id("_id")
            .map(|o| o.to_hex())
            .unwrap_or_default()
    });
    Ok(docs)
}

#[tokio::test]
async fn import_counts_only_new_ids() -> TestResult {
    let db = setup().await;
    let v = vista(&db, TASK);
    let existing = [ObjectId::new(), ObjectId::new()];
    for id in existing {
        v.insert_value(id.to_hex(), &row("old", "open")).await?;
    }
    let before = stored(&db, "tasks").await?;

    let new = [ObjectId::new(), ObjectId::new(), ObjectId::new()];
    let mut rows: Vec<_> = existing
        .iter()
        .map(|id| (*id, row("changed", "done")))
        .collect();
    rows.extend(new.iter().map(|id| (*id, row("fresh", "open"))));

    assert_eq!(v.import_values(&batch(rows)).await?, 3);

    let after = stored(&db, "tasks").await?;
    assert_eq!(after.len(), 5);
    for doc in &before {
        assert!(after.contains(doc), "existing doc changed: {doc:?}");
    }
    for id in new {
        let doc = after
            .iter()
            .find(|d| d.get_object_id("_id") == Ok(id))
            .unwrap();
        assert_eq!(doc.get_str("title")?, "fresh");
    }

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn import_runs_insert_hooks_for_new_records_only() -> TestResult {
    let db = setup().await;
    let after_calls = Arc::new(AtomicUsize::new(0));
    let calls = after_calls.clone();
    let before: BeforeFn<MongoDB> = Arc::new(|record, _table| {
        Box::pin(async move {
            record.insert(
                "status".to_string(),
                AnyMongoType::new("defaulted".to_string()),
            );
            Ok(())
        })
    });
    let after: AfterFn<MongoDB> = Arc::new(move |_id, _record, _table| {
        let calls = calls.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    });
    let table = Table::<MongoDB, EmptyEntity>::new("tasks", db.clone())
        .with_column_of::<String>("title")
        .with_column_of::<String>("status")
        .with_hook(Hook::BeforeInsert(Phase::Populate, before))
        .with_hook(Hook::AfterInsert(after));
    let v = db.vista_factory().from_table(table)?;

    let existing = ObjectId::new();
    db.doc_collection("tasks")
        .insert_one(doc! { "_id": existing, "title": "old", "status": "kept" })
        .await?;
    let new = [ObjectId::new(), ObjectId::new()];
    let mut rows = vec![(existing, row("changed", "x"))];
    rows.extend(new.iter().map(|id| (*id, row("fresh", "x"))));

    assert_eq!(v.import_values(&batch(rows)).await?, 2);

    let docs = stored(&db, "tasks").await?;
    for id in new {
        let doc = docs
            .iter()
            .find(|d| d.get_object_id("_id") == Ok(id))
            .unwrap();
        assert_eq!(doc.get_str("status")?, "defaulted");
    }
    let old = docs
        .iter()
        .find(|d| d.get_object_id("_id") == Ok(existing))
        .unwrap();
    assert_eq!(old.get_str("status")?, "kept");
    assert_eq!(after_calls.load(Ordering::SeqCst), 2);

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn import_fills_set_columns_the_record_lacks() -> TestResult {
    let db = setup().await;
    let mut open = vista(&db, TASK);
    open.add_condition_eq("status", CborValue::Text("open".into()))?;
    let id = ObjectId::new();
    let mut record = Record::new();
    record.insert("title".to_string(), CborValue::Text("no status".into()));

    assert_eq!(open.import_values(&batch(vec![(id, record)])).await?, 1);

    let docs = stored(&db, "tasks").await?;
    assert_eq!(docs[0].get_str("status")?, "open");

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn import_conflicts_on_id_held_outside_the_set() -> TestResult {
    let db = setup().await;
    let all = vista(&db, TASK);
    let closed = ObjectId::new();
    all.insert_value(closed.to_hex(), &row("closed one", "closed"))
        .await?;
    let before = stored(&db, "tasks").await?;

    let mut open = vista(&db, TASK);
    open.add_condition_eq("status", CborValue::Text("open".into()))?;
    let rows = batch(vec![
        (ObjectId::new(), row("a", "open")),
        (closed, row("b", "open")),
    ]);

    let Err(err) = open.import_values(&rows).await else {
        panic!("import must conflict");
    };
    assert!(err.is_conflict(), "wrong error kind: {err}");
    assert_eq!(stored(&db, "tasks").await?, before);

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn import_rolls_back_on_out_of_set_row() -> TestResult {
    let db = setup().await;
    let all = vista(&db, TASK);
    all.insert_value(ObjectId::new().to_hex(), &row("kept", "open"))
        .await?;
    let before = stored(&db, "tasks").await?;

    let mut open = vista(&db, TASK);
    open.add_condition_eq("status", CborValue::Text("open".into()))?;
    let rows = batch(vec![
        (ObjectId::new(), row("a", "open")),
        (ObjectId::new(), row("b", "closed")),
        (ObjectId::new(), row("c", "open")),
    ]);

    let Err(err) = open.import_values(&rows).await else {
        panic!("import must reject the out-of-set row");
    };
    assert!(err.is_conflict(), "wrong error kind: {err}");
    assert_eq!(stored(&db, "tasks").await?, before);

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn import_rolls_back_on_server_error() -> TestResult {
    let db = setup().await;
    let v = vista(&db, TASK);
    v.insert_value(ObjectId::new().to_hex(), &sku_row("existing"))
        .await?;
    db.doc_collection("tasks")
        .create_index(
            IndexModel::builder()
                .keys(doc! { "sku": 1 })
                .options(IndexOptions::builder().unique(true).build())
                .build(),
        )
        .await?;
    let before = stored(&db, "tasks").await?;

    let rows = batch(vec![
        (ObjectId::new(), sku_row("a")),
        (ObjectId::new(), sku_row("dup")),
        (ObjectId::new(), sku_row("dup")),
    ]);

    assert!(v.import_values(&rows).await.is_err());
    assert_eq!(stored(&db, "tasks").await?, before);

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn import_keeps_nested_paths() -> TestResult {
    let db = setup().await;
    let v = vista(&db, CLIENT);
    let id = ObjectId::new();
    let mut record = Record::new();
    record.insert("name".to_string(), CborValue::Text("Doc".into()));
    record.insert("city".to_string(), CborValue::Text("Hill Valley".into()));

    assert_eq!(v.import_values(&batch(vec![(id, record)])).await?, 1);

    let docs = stored(&db, "clients").await?;
    assert_eq!(docs.len(), 1);
    assert_eq!(
        docs[0].get_document("address")?.get_str("city")?,
        "Hill Valley"
    );
    assert!(docs[0].get("city").is_none());

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn rows_copied_from_a_read_keep_object_id_keys() -> TestResult {
    let db = setup().await;
    let all = vista(&db, TASK);
    let ids = [ObjectId::new(), ObjectId::new()];
    for id in ids {
        all.insert_value(id.to_hex(), &row("t", "open")).await?;
    }
    // A read row carries its `_id` as hex text.
    let rows = all.list_values().await?;
    let first = rows.get(&ids[0].to_hex()).expect("row read back").clone();
    assert!(matches!(first.get("_id"), Some(CborValue::Text(_))));

    let copies = vista(
        &db,
        &TASK.replace("collection: tasks", "collection: copies"),
    );
    copies.insert_value(ids[0].to_hex(), &first).await?;

    let mut open = vista(
        &db,
        &TASK.replace("collection: tasks", "collection: imported"),
    );
    open.add_condition_eq("status", CborValue::Text("open".into()))?;
    assert_eq!(open.import_values(&rows).await?, 2);

    for collection in ["copies", "imported"] {
        let docs = stored(&db, collection).await?;
        assert!(!docs.is_empty(), "{collection} is empty");
        for doc in docs {
            assert!(doc.get_object_id("_id").is_ok(), "{collection}: {doc:?}");
        }
    }

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn capabilities_without_transactions_hide_import() -> TestResult {
    let db = setup().await;
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(TASK)?;

    let off = MongoVistaFactory::new(db.clone())
        .with_change_streams(false)
        .build_from_spec(spec.clone())?;
    assert!(!off.capabilities().can_import);

    let default = MongoVistaFactory::new(db.clone()).build_from_spec(spec)?;
    assert!(default.capabilities().can_import);

    teardown(&db).await;
    Ok(())
}

#[tokio::test]
async fn import_empty_is_zero() -> TestResult {
    let db = setup().await;
    let v = vista(&db, TASK);
    assert_eq!(v.import_values(&IndexMap::new()).await?, 0);
    teardown(&db).await;
    Ok(())
}
