//! Operator push-down through the vista against a live MongoDB. Set
//! `MONGODB_URL` (defaults to `mongodb://localhost:27017`). Each test runs
//! against a fresh randomised database and drops it on the way out.

#![cfg(feature = "vista")]

use std::error::Error;

use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_mongodb::{MongoCondition, MongoDB};
use vantage_vista::{Capability, FilterOp, NativeCondition, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_vista_filters_{}", ObjectId::new().to_hex());
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

const PRODUCT: &str = r#"
name: product
columns:
  _id: { type: object_id, flags: [id] }
  name: { type: string, flags: [title] }
  price: { type: int }
mongo:
  collection: products
"#;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

async fn seed(db: &MongoDB) -> std::result::Result<Vec<ObjectId>, Box<dyn Error>> {
    let ids: Vec<ObjectId> = (0..3).map(|_| ObjectId::new()).collect();
    db.doc_collection("products")
        .insert_many(vec![
            bson::doc! { "_id": ids[0], "name": "Dark Chocolate", "price": 120 },
            bson::doc! { "_id": ids[1], "name": "Vanilla Cupcake", "price": 100 },
            bson::doc! { "_id": ids[2], "name": "Plain Bread", "price": 40 },
        ])
        .await?;
    Ok(ids)
}

fn products(db: &MongoDB) -> vantage_vista::Vista {
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(PRODUCT).expect("spec yaml");
    db.vista_factory().build_from_spec(spec).expect("vista")
}

fn names(rows: &indexmap::IndexMap<String, vantage_types::Record<CborValue>>) -> Vec<String> {
    let mut out: Vec<String> = rows
        .values()
        .filter_map(|r| match r.get("name") {
            Some(CborValue::Text(s)) => Some(s.clone()),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

#[tokio::test]
async fn vista_op_conditions_push_down() -> TestResult {
    let (db, name) = setup().await;
    let ids = seed(&db).await?;

    let mut v = products(&db);
    v.add_condition("price", FilterOp::Gte, int(100))?;
    assert_eq!(v.count().await?, 2);

    let mut v = products(&db);
    v.add_condition("name", FilterOp::Like, text("CHOC"))?;
    assert_eq!(names(&v.list_values().await?), vec!["Dark Chocolate"]);

    let mut v = products(&db);
    v.add_condition(
        "name",
        FilterOp::NotInSet,
        CborValue::Array(vec![text("Plain Bread"), text("Dark Chocolate")]),
    )?;
    assert_eq!(names(&v.list_values().await?), vec!["Vanilla Cupcake"]);

    let mut v = products(&db);
    v.add_condition("price", FilterOp::Lt, int(100))?;
    assert_eq!(names(&v.list_values().await?), vec!["Plain Bread"]);

    let mut v = products(&db);
    v.add_condition("price", FilterOp::Ne, int(100))?;
    assert_eq!(v.count().await?, 2);

    // Hex strings find documents whose `_id` is an ObjectId.
    let mut v = products(&db);
    v.add_condition(
        "_id",
        FilterOp::InSet,
        CborValue::Array(vec![text(&ids[0].to_hex()), text(&ids[2].to_hex())]),
    )?;
    assert_eq!(
        names(&v.list_values().await?),
        vec!["Dark Chocolate", "Plain Bread"]
    );

    let mut v = products(&db);
    v.add_condition("_id", FilterOp::Eq, text(&ids[1].to_hex()))?;
    assert_eq!(names(&v.list_values().await?), vec!["Vanilla Cupcake"]);

    let mut v = products(&db);
    v.add_condition("_id", FilterOp::Ne, text(&ids[1].to_hex()))?;
    assert_eq!(v.count().await?, 2);

    let mut v = products(&db);
    assert!(v.add_condition("nope", FilterOp::Eq, int(1)).is_err());
    assert!(v.add_condition("price", FilterOp::InSet, int(1)).is_err());

    teardown(&db, &name).await;
    Ok(())
}

const CLIENT: &str = r#"
name: client
columns:
  _id: { type: object_id, flags: [id] }
  name: { type: string, flags: [title, searchable] }
  price: { type: int }
  city:
    type: string
    flags: [searchable]
    mongo:
      nested_path: address.city
mongo:
  collection: clients
"#;

const CLIENT_NO_SEARCH: &str = r#"
name: client
columns:
  _id: { type: object_id, flags: [id] }
  name: { type: string, flags: [title] }
  price: { type: int }
mongo:
  collection: clients
"#;

async fn seed_clients(db: &MongoDB) -> TestResult {
    db.doc_collection("clients")
        .insert_many(vec![
            bson::doc! { "_id": ObjectId::new(), "name": "Marty", "price": 120,
            "address": { "city": "Hill Valley" } },
            bson::doc! { "_id": ObjectId::new(), "name": "Biff", "price": 5,
            "address": { "city": "Springfield" } },
        ])
        .await?;
    Ok(())
}

fn clients(db: &MongoDB, yaml: &str) -> vantage_vista::Vista {
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(yaml).expect("spec yaml");
    db.vista_factory().build_from_spec(spec).expect("vista")
}

#[tokio::test]
async fn vista_search_uses_searchable_paths() -> TestResult {
    let (db, name) = setup().await;
    seed_clients(&db).await?;

    let mut v = clients(&db, CLIENT);
    v.set_search(Some("hill"))?;
    assert_eq!(names(&v.list_values().await?), vec!["Marty"]);

    // `price` is not searchable, so its digits never match.
    v.set_search(Some("12"))?;
    assert_eq!(v.list_values().await?.len(), 0);

    v.set_search(Some("biff"))?;
    assert_eq!(names(&v.list_values().await?), vec!["Biff"]);

    v.set_search(None)?;
    assert_eq!(v.list_values().await?.len(), 2);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_search_without_searchable_columns_matches_nothing() -> TestResult {
    let (db, name) = setup().await;
    seed_clients(&db).await?;

    let mut v = clients(&db, CLIENT_NO_SEARCH);
    v.set_search(Some("marty"))?;
    assert_eq!(v.list_values().await?.len(), 0);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn native_condition_document_narrows_list() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let mut v = products(&db);
    v.add_native_condition(NativeCondition::new(
        bson::doc! { "price": { "$gt": 50 }, "name": { "$regex": "^Dark" } },
        "dark over 50",
    ))?;
    assert_eq!(names(&v.list_values().await?), vec!["Dark Chocolate"]);
    assert_eq!(v.query().native, vec!["dark over 50".to_string()]);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn native_condition_mongo_condition_narrows_list() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let mut v = products(&db);
    v.add_native_condition(NativeCondition::new(
        MongoCondition::And(vec![
            bson::doc! { "price": { "$gte": 100 } }.into(),
            bson::doc! { "name": { "$ne": "Dark Chocolate" } }.into(),
        ]),
        "pricey, not dark",
    ))?;
    assert_eq!(names(&v.list_values().await?), vec!["Vanilla Cupcake"]);
    assert_eq!(v.count().await?, 1);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn native_condition_wrong_type_errors() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let mut v = products(&db);
    let err = v
        .add_native_condition(NativeCondition::new("price > 50".to_string(), "text"))
        .expect_err("a string is not a Mongo condition");
    assert!(err.is_incorrect_usage(), "error: {err:?}");
    assert!(
        err.to_string().contains("bson::Document or MongoCondition"),
        "error: {err}"
    );
    assert_eq!(v.count().await?, 3);
    assert!(v.query().native.is_empty());

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn vista_capabilities_advertise_operators_and_traversal() -> TestResult {
    let (db, name) = setup().await;
    let caps = *products(&db).capabilities();
    assert!(caps.has(Capability::FilterOps));
    assert!(caps.has(Capability::NativeConditions));
    assert!(caps.has(Capability::Traverse));
    teardown(&db, &name).await;
    Ok(())
}
