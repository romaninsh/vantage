//! Rhai scripting over MongoDB vistas, against a live MongoDB: `modify:`
//! scripts, `rhai:` pipeline views and scripted references. Set `MONGODB_URL`
//! (defaults to `mongodb://localhost:27017`). Each test runs against a fresh
//! randomised database and drops it on the way out.
//!
//! - this file: the fixtures, seed and shared specs;
//! - `modify`: `modify:` scripts on a built vista;
//! - `pipelines`: `rhai:` pipeline views;
//! - `refs`: scripted references and the capabilities they advertise;
//! - `script_tables`: `table(name)` in a reference script, with and without
//!   a resolver.

#![cfg(feature = "rhai")]

mod modify;
mod pipelines;
mod refs;
mod script_tables;

use std::error::Error;
use std::sync::Arc;

use bson::doc;
use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_mongodb::MongoDB;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_types::Record;
use vantage_vista::SpecResolver;

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_vista_rhai_{}", ObjectId::new().to_hex());
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

fn spec(yaml: &str) -> MongoVistaSpec {
    serde_yaml_ng::from_str(yaml).expect("spec yaml")
}

fn resolver(specs: Vec<MongoVistaSpec>) -> Arc<dyn SpecResolver<MongoVistaSpec>> {
    let map: IndexMap<String, MongoVistaSpec> =
        specs.into_iter().map(|s| (s.name.clone(), s)).collect();
    Arc::new(move |name: &str| map.get(name).cloned())
}

fn text(row: &Record<CborValue>, key: &str) -> Option<String> {
    match row.get(key) {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

async fn seed_products(db: &MongoDB) -> TestResult {
    let rows = [
        ("cake", 150),
        ("bread", 5),
        ("cake", 20),
        ("candy", 300),
        ("bread", 7),
        ("cake", 30),
    ];
    let docs = rows
        .into_iter()
        .map(|(category, price)| doc! { "_id": ObjectId::new(), "category": category, "price": price })
        .collect::<Vec<_>>();
    db.doc_collection("products").insert_many(docs).await?;
    Ok(())
}

const PLAIN_PRODUCT: &str = r#"
name: product
columns:
  _id: { type: object_id, flags: [id] }
  category: { type: string }
  price: { type: int }
mongo:
  collection: products
"#;

const BY_CATEGORY: &str = r#"
name: category_counts
columns:
  _id: { type: string, flags: [id] }
  n: { type: int }
mongo:
  collection: products
  rhai: |
    [
      #{"$match": #{category: args.cat}},
      #{"$group": #{_id: "$category", n: #{"$sum": 1}}},
    ]
"#;

const CLIENT: &str = r#"
name: client
columns:
  _id: { type: object_id, flags: [id] }
  name: { type: string, flags: [title] }
mongo:
  collection: clients
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client_id
    mongo:
      rhai: |
        table("order").where("client_id", me._id).where("status", "open")
"#;

const ORDER: &str = r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: object_id }
  item: { type: string, flags: [title] }
  status: { type: string }
mongo:
  collection: orders
"#;

/// The `CLIENT` spec with its `orders` reference script replaced.
fn client_with_script(script: &str) -> String {
    CLIENT.replace(
        r#"table("order").where("client_id", me._id).where("status", "open")"#,
        script,
    )
}

fn any_client_row() -> Record<CborValue> {
    let mut row = Record::new();
    row.insert("_id".to_string(), CborValue::Text(ObjectId::new().to_hex()));
    row
}
