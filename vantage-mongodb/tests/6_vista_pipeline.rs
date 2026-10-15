//! Read-only vistas over a MongoDB aggregation pipeline, against a live
//! MongoDB. Set `MONGODB_URL` (defaults to `mongodb://localhost:27017`). Each
//! test runs against a fresh randomised database and drops it on the way out.

#![cfg(feature = "vista")]

use std::error::Error;

use bson::doc;
use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_mongodb::vista::{DriverBlockArgs, MongoBlock, MongoVistaSpec};
use vantage_mongodb::{AnyMongoType, MongoDB, MongoId, MongoView};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Capability, FilterOp, NativeCondition, SortDirection, VistaFactory, contract};

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_vista_pipeline_{}", ObjectId::new().to_hex());
    let db = MongoDB::connect(&mongo_url(), &db_name)
        .await
        .expect("connect mongo");
    (db, db_name)
}

async fn teardown(db: &MongoDB, db_name: &str) {
    db.database()
        .drop()
        .await
        .unwrap_or_else(|e| eprintln!("drop {db_name}: {e}"));
}

/// Category totals: cake 60 (3), drink 25 (1), bread 12 (2), candy 7 (2).
async fn seed(db: &MongoDB) -> TestResult {
    let rows = [
        ("cake", 10),
        ("bread", 5),
        ("cake", 20),
        ("candy", 3),
        ("drink", 25),
        ("bread", 7),
        ("cake", 30),
        ("candy", 4),
    ];
    let docs = rows
        .into_iter()
        .map(|(category, price)| doc! { "_id": ObjectId::new(), "category": category, "price": price })
        .collect::<Vec<_>>();
    db.doc_collection("products").insert_many(docs).await?;
    Ok(())
}

/// Documents stored in the underlying collection.
async fn stored(db: &MongoDB) -> std::result::Result<u64, Box<dyn Error>> {
    Ok(db
        .doc_collection("products")
        .count_documents(doc! {})
        .await?)
}

const CATEGORY_TOTALS: &str = r#"
name: category_totals
columns:
  _id: { type: string, flags: [id] }
  category: { type: string, flags: [title, searchable] }
  total: { type: int }
  n: { type: int }
mongo:
  collection: products
  pipeline:
    - $group: { _id: "$category", total: { $sum: "$price" }, n: { $sum: 1 } }
    - $addFields: { category: "$_id" }
"#;

fn totals(db: &MongoDB) -> vantage_vista::Vista {
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(CATEGORY_TOTALS).expect("spec yaml");
    db.vista_factory().build_from_spec(spec).expect("vista")
}

fn ids(rows: &[(String, Record<CborValue>)]) -> Vec<&str> {
    rows.iter().map(|(id, _)| id.as_str()).collect()
}

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn total_of(row: &Record<CborValue>) -> i128 {
    match row.get("total") {
        Some(CborValue::Integer(i)) => i128::from(*i),
        other => panic!("total is not an integer: {other:?}"),
    }
}

#[tokio::test]
async fn pipeline_view_lists_counts_filters_orders_pages() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let v = totals(&db);
    let rows = v.list_values().await?;
    let mut keys: Vec<&str> = rows.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, vec!["bread", "cake", "candy", "drink"]);
    assert_eq!(total_of(&rows["cake"]), 60);
    assert_eq!(
        rows["cake"].get("category"),
        Some(&CborValue::Text("cake".into()))
    );
    assert_eq!(v.count().await?, 4);

    let mut v = totals(&db);
    v.add_condition("total", FilterOp::Gte, int(12))?;
    v.set_order(Some(("total", SortDirection::Descending)))?;
    assert_eq!(v.count().await?, 3);

    assert_eq!(
        ids(&v.fetch_window(0, 2).await?.rows),
        vec!["cake", "drink"]
    );
    assert_eq!(ids(&v.fetch_window(2, 5).await?.rows), vec!["bread"]);

    let (first, token) = v.fetch_cursor(None, 2).await?;
    assert_eq!(ids(&first.rows), vec!["cake", "drink"]);
    let (second, token) = v.fetch_cursor(token, 2).await?;
    assert_eq!(ids(&second.rows), vec!["bread"]);
    assert!(token.is_none());

    let mut v = totals(&db);
    v.set_search(Some("CA"))?;
    let mut found: Vec<String> = v.list_values().await?.into_keys().collect();
    found.sort();
    assert_eq!(found, vec!["cake", "candy"]);
    assert_eq!(v.count().await?, 2);

    let preview = v.preview().query.expect("a rendered query");
    assert!(preview.contains("$group"), "preview: {preview}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn pipeline_view_get_value_by_group_key() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let v = totals(&db);
    let cake = v.get_value("cake".to_string()).await?.expect("cake row");
    assert_eq!(total_of(&cake), 60);
    assert!(v.get_value("nope".to_string()).await?.is_none());

    // The view's conditions confine the lookup.
    let mut v = totals(&db);
    v.add_condition("total", FilterOp::Lt, int(20))?;
    assert!(v.get_value("cake".to_string()).await?.is_none());
    assert!(v.get_value("bread".to_string()).await?.is_some());

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn pipeline_view_is_read_only() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let v = totals(&db);
    let caps = *v.capabilities();
    for capability in [
        Capability::Order,
        Capability::Search,
        Capability::FilterOps,
        Capability::NativeConditions,
        Capability::Window,
        Capability::Cursor,
        Capability::Traverse,
    ] {
        assert!(caps.has(capability), "{capability} should be on");
    }
    for capability in [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::Import,
        Capability::Subscribe,
        Capability::ConfineWrites,
    ] {
        assert!(!caps.has(capability), "{capability} should be off");
    }

    let mut record = Record::new();
    record.insert("total".to_string(), int(1));
    let err = v
        .insert_value("pie".to_string(), &record)
        .await
        .expect_err("insert on a view");
    assert!(err.is_unsupported(), "error: {err:?}");
    assert!(v.delete("cake".to_string()).await.is_err());
    assert_eq!(stored(&db).await?, 8);
    contract::check_advertised(&v).await;

    teardown(&db, &name).await;
    Ok(())
}

/// A typed table over a view-carrying `MongoDB`: aggregates run after the
/// view's stages, and every write is refused.
#[tokio::test]
async fn pipeline_view_typed_table_aggregates_and_refuses_writes() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let view_db = db.clone().with_view(MongoView::new(
        "products",
        vec![doc! { "$group": { "_id": "$category", "total": { "$sum": "$price" } } }],
    ));
    let table = Table::<MongoDB, EmptyEntity>::new("products", view_db.clone())
        .with_id_column("_id")
        .with_column_of::<i64>("total");

    assert_eq!(table.get_count().await?, 4);
    let max = view_db.get_table_max(&table, &table["total"]).await?;
    assert_eq!(max.try_get::<i64>(), Some(60));
    let sum = view_db.get_table_sum(&table, &table["total"]).await?;
    assert_eq!(sum.try_get::<i64>(), Some(104));

    let mut empty = table.clone();
    empty.add_condition(doc! { "total": { "$gt": 1000 } });
    assert_eq!(empty.get_count().await?, 0);
    let sum = view_db.get_table_sum(&empty, &empty["total"]).await?;
    assert_eq!(sum.try_get::<i64>(), Some(0));

    let id = MongoId::String("cake".into());
    let mut record = Record::new();
    record.insert("total".to_string(), AnyMongoType::untyped(1_i64.into()));
    let refusals = [
        view_db
            .delete_table_all_values(&table)
            .await
            .expect_err("delete_all on a view"),
        view_db
            .replace_table_value(&table, &id, &record)
            .await
            .expect_err("replace on a view"),
        view_db
            .patch_table_value(&table, &id, &record)
            .await
            .expect_err("patch on a view"),
    ];
    for err in refusals {
        assert!(err.to_string().contains("read-only"), "error: {err}");
    }
    assert_eq!(stored(&db).await?, 8);

    teardown(&db, &name).await;
    Ok(())
}

/// A view with no stages reads its collection's documents, whatever the
/// table is named, through every read.
#[tokio::test]
async fn pipeline_view_without_stages_reads_view_collection() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let view_db = db.clone().with_view(MongoView::new("products", vec![]));
    let table = Table::<MongoDB, EmptyEntity>::new("elsewhere", view_db.clone())
        .with_id_column("_id")
        .with_column_of::<String>("category");

    assert_eq!(table.get_count().await?, 8);
    assert_eq!(table.list_values().await?.len(), 8);
    let (id, _) = table.get_some_value().await?.expect("some row");
    assert!(table.get_value(id).await?.is_some());

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn a_new_view_replaces_the_old_one_and_leaves_the_original_handle_alone() -> TestResult {
    let (db, name) = setup().await;
    assert!(db.view().is_none());

    let first = db.clone().with_view(MongoView::new(
        "products",
        vec![doc! { "$match": { "a": 1 } }],
    ));
    let second = first.clone().with_view(MongoView::new("orders", vec![]));

    let view = first.view().expect("first view");
    assert_eq!(view.collection, "products");
    assert_eq!(view.stages, vec![doc! { "$match": { "a": 1 } }]);
    let view = second.view().expect("second view");
    assert_eq!(view.collection, "orders");
    assert!(view.stages.is_empty());
    assert!(db.view().is_none());

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn the_server_view_flag_is_off_by_default_and_composes_with_a_pipeline_view() -> TestResult {
    let (db, name) = setup().await;
    assert!(!db.is_server_view());

    let flagged = db.clone().with_server_view(true);
    assert!(flagged.is_server_view());
    assert!(!db.is_server_view());

    let both = flagged.with_view(MongoView::new("products", vec![]));
    assert!(both.is_server_view() && both.view().is_some());
    assert!(!both.with_server_view(false).is_server_view());

    teardown(&db, &name).await;
    Ok(())
}

fn block_with(stages: Vec<serde_json::Value>) -> MongoBlock {
    MongoBlock {
        pipeline: Some(stages),
        ..MongoBlock::default()
    }
}

#[test]
fn a_block_without_a_pipeline_has_no_stages() {
    assert!(MongoBlock::default().pipeline_stages().unwrap().is_none());
}

#[test]
fn yaml_pipeline_stages_parse_into_bson_documents_in_written_order() {
    let yaml = r#"
name: totals
columns:
  _id: { type: string, flags: [id] }
mongo:
  collection: products
  pipeline:
    - $group: { _id: "$category", total: { $sum: "$price" } }
    - $sort: { total: -1 }
"#;
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let stages = spec
        .driver
        .mongo
        .unwrap()
        .pipeline_stages()
        .unwrap()
        .unwrap();
    assert_eq!(
        stages,
        vec![
            doc! { "$group": { "_id": "$category", "total": { "$sum": "$price" } } },
            doc! { "$sort": { "total": -1_i64 } },
        ]
    );
}

#[test]
fn an_empty_pipeline_is_refused() {
    let msg = block_with(vec![])
        .pipeline_stages()
        .unwrap_err()
        .to_string();
    assert!(msg.contains("at least one stage"), "{msg}");
}

#[test]
fn a_stage_must_be_a_map_with_one_dollar_operator_and_the_error_names_its_index() {
    for bad in [
        serde_json::json!({ "group": { "_id": 1 } }),
        serde_json::json!({ "$match": {}, "$sort": { "a": 1 } }),
        serde_json::json!(["$match"]),
        serde_json::json!("$match"),
        serde_json::json!({}),
    ] {
        let block = block_with(vec![serde_json::json!({ "$match": {} }), bad.clone()]);
        let msg = block.pipeline_stages().unwrap_err().to_string();
        assert!(msg.contains("pipeline stage"), "{bad}: {msg}");
        assert!(msg.contains("stage: 1"), "{bad}: {msg}");
    }
}

#[test]
fn a_pipeline_cannot_write_with_out_or_merge() {
    for writer in [
        serde_json::json!({ "$out": "copy" }),
        serde_json::json!({ "$merge": { "into": "copy" } }),
    ] {
        let block = block_with(vec![serde_json::json!({ "$match": {} }), writer.clone()]);
        let msg = block.pipeline_stages().unwrap_err().to_string();
        assert!(msg.contains("read-only"), "{writer}: {msg}");
        assert!(msg.contains("stage: 1"), "{writer}: {msg}");
    }
}

#[test]
fn driver_block_args_reads_the_args_off_a_spec_and_is_empty_without_a_block() {
    let mut spec: MongoVistaSpec = serde_yaml_ng::from_str(CATEGORY_TOTALS).unwrap();
    assert!(spec.driver_block_args().is_empty());
    spec.driver.mongo.as_mut().unwrap().args = vec![("cat".into(), "cake".into())];
    assert_eq!(
        spec.driver_block_args(),
        vec![("cat".to_string(), "cake".to_string())]
    );

    let bare: MongoVistaSpec =
        serde_yaml_ng::from_str("name: bare\ncolumns:\n  _id: { type: string, flags: [id] }\n")
            .unwrap();
    assert!(bare.driver_block_args().is_empty());
}

#[tokio::test]
async fn pipeline_view_rejects_bad_stage() -> TestResult {
    let (db, name) = setup().await;
    let yaml = r#"
name: bad
columns:
  _id: { type: string, flags: [id] }
mongo:
  collection: products
  pipeline:
    - $match: { a: 1 }
    - group: { _id: "$a" }
"#;
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(yaml)?;
    let Err(err) = db.vista_factory().build_from_spec(spec) else {
        panic!("a stage without a `$` operator must be rejected");
    };
    assert!(err.to_string().contains("stage: 1"), "error: {err}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn pipeline_view_accepts_native_condition() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let mut v = totals(&db);
    v.add_native_condition(NativeCondition::new(doc! { "n": { "$gte": 2 } }, "n >= 2"))?;
    assert_eq!(v.count().await?, 3);

    teardown(&db, &name).await;
    Ok(())
}
