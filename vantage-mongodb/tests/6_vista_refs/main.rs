//! YAML-vista reference traversal against a live MongoDB. Set `MONGODB_URL`
//! (defaults to `mongodb://localhost:27017`). Each test runs against a fresh
//! randomised database and drops it on the way out.
//!
//! - this file: the fixtures and seed;
//! - `traverse`: has-one and has-many traversal, bare targets, nested paths;
//! - `object_ids`: ObjectId foreign keys read, written and inserted;
//! - `refusals`: reference keys a traversal can't follow.

#![cfg(feature = "vista")]

mod object_ids;
mod refusals;
mod traverse;

use std::error::Error;
use std::sync::Arc;

use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_mongodb::MongoDB;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_vista::SpecResolver;

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_vista_refs_{}", ObjectId::new().to_hex());
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

fn resolver(specs: Vec<MongoVistaSpec>) -> Arc<dyn SpecResolver<MongoVistaSpec>> {
    let map: IndexMap<String, MongoVistaSpec> =
        specs.into_iter().map(|s| (s.name.clone(), s)).collect();
    Arc::new(move |name: &str| map.get(name).cloned())
}

fn spec(yaml: &str) -> MongoVistaSpec {
    serde_yaml_ng::from_str(yaml).expect("spec yaml")
}

const CLIENT: &str = r#"
name: client
columns:
  _id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
  city:
    type: string
    mongo:
      nested_path: address.city
mongo:
  collection: clients
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client_id
"#;

const ORDER: &str = r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: string }
  item: { type: string, flags: [title] }
mongo:
  collection: orders
references:
  client:
    table: client
    kind: has_one
    foreign_key: client_id
"#;

async fn seed(db: &MongoDB) -> TestResult {
    db.collection::<bson::Document>("clients")
        .insert_many(vec![
            bson::doc! { "_id": "c1", "name": "Marty", "address": { "city": "Hill Valley" } },
            bson::doc! { "_id": "c2", "name": "Doc", "address": { "city": "Twin Pines" } },
        ])
        .await?;
    db.collection::<bson::Document>("orders")
        .insert_many(vec![
            bson::doc! { "_id": "o1", "client_id": "c1", "item": "skateboard" },
            bson::doc! { "_id": "o2", "client_id": "c1", "item": "almanac" },
            bson::doc! { "_id": "o3", "client_id": "c2", "item": "flux capacitor" },
        ])
        .await?;
    Ok(())
}

fn text(row: &vantage_types::Record<CborValue>, key: &str) -> Option<String> {
    match row.get(key) {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    }
}
