//! Dotted join columns (`client.name`) on YAML vistas, read through
//! `$lookup`, against a live MongoDB. Set `MONGODB_URL` (defaults to
//! `mongodb://localhost:27017`). Each test runs against a fresh randomised
//! database and drops it on the way out.
//!
//! - this file: the fixtures and seed;
//! - `reads`: joined values, filters, order, paging, aggregates and watch;
//! - `writes`: the joined value stripped on write, confinement by a join;
//! - `specs`: specs that may not declare a join.

#![cfg(feature = "vista")]

mod reads;
mod specs;
mod writes;

use std::error::Error;
use std::sync::Arc;

use bson::doc;
use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_mongodb::MongoDB;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_types::Record;
use vantage_vista::{SpecResolver, Vista, VistaFactory};

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn setup() -> (MongoDB, String) {
    let db_name = format!("vantage_vista_joins_{}", ObjectId::new().to_hex());
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

const COUNTRY: &str = r#"
name: country
columns:
  _id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
mongo:
  collection: countries
"#;

const CLIENT: &str = r#"
name: client
columns:
  _id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
  country_id: { type: string }
mongo:
  collection: clients
references:
  country:
    table: country
    kind: has_one
    foreign_key: country_id
"#;

const ORDER: &str = r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: string }
  item: { type: string, flags: [title] }
  client.name: { type: string, flags: [searchable] }
  client.country.name: { type: string }
mongo:
  collection: orders
references:
  client:
    table: client
    kind: has_one
    foreign_key: client_id
"#;

fn resolver() -> Arc<dyn SpecResolver<MongoVistaSpec>> {
    let map: IndexMap<String, MongoVistaSpec> = [COUNTRY, CLIENT, ORDER]
        .into_iter()
        .map(|y| {
            let s = spec(y);
            (s.name.clone(), s)
        })
        .collect();
    Arc::new(move |name: &str| map.get(name).cloned())
}

fn orders(db: &MongoDB) -> Vista {
    db.vista_factory()
        .with_resolver(resolver())
        .build_from_spec(spec(ORDER))
        .expect("orders vista")
}

struct Ids {
    marty: ObjectId,
}

/// Marty (USA) has o1 and o2, Doc (Canada) has o3, o4 points at no client.
async fn seed(db: &MongoDB) -> std::result::Result<Ids, Box<dyn Error>> {
    let (usa, canada) = (ObjectId::new(), ObjectId::new());
    let (marty, doc_brown) = (ObjectId::new(), ObjectId::new());
    db.doc_collection("countries")
        .insert_many(vec![
            doc! { "_id": usa, "name": "USA" },
            doc! { "_id": canada, "name": "Canada" },
        ])
        .await?;
    db.doc_collection("clients")
        .insert_many(vec![
            doc! { "_id": marty, "name": "Marty", "country_id": usa },
            doc! { "_id": doc_brown, "name": "Doc", "country_id": canada },
        ])
        .await?;
    db.doc_collection("orders")
        .insert_many(vec![
            doc! { "_id": "o1", "client_id": marty, "item": "skateboard" },
            doc! { "_id": "o2", "client_id": marty, "item": "almanac" },
            doc! { "_id": "o3", "client_id": doc_brown, "item": "flux capacitor" },
            doc! { "_id": "o4", "client_id": ObjectId::new(), "item": "hoverboard" },
        ])
        .await?;
    Ok(Ids { marty })
}

fn text(row: &Record<CborValue>, key: &str) -> Option<String> {
    match row.get(key) {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

fn column(rows: &IndexMap<String, Record<CborValue>>, key: &str) -> Vec<(String, Option<String>)> {
    rows.iter()
        .map(|(id, r)| (id.clone(), text(r, key)))
        .collect()
}

fn pair(id: &str, v: Option<&str>) -> (String, Option<String>) {
    (id.to_string(), v.map(str::to_string))
}
