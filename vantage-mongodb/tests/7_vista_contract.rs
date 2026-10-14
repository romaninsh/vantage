//! `vantage_vista::contract` over a live MongoDB. Set `MONGODB_URL`
//! (defaults to `mongodb://localhost:27017`). Each test runs against a fresh
//! randomised database and drops it on the way out.

#![cfg(feature = "vista")]

use std::sync::Arc;

use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_mongodb::{MongoDB, MongoVistaFactory};
use vantage_vista::{SpecResolver, VistaFactory, contract};

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

const CLIENT: &str = r#"
name: client
columns:
  _id: { type: string, flags: [id, orderable] }
  name: { type: string, flags: [title, searchable] }
  address: {}
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
  _id: { type: string, flags: [id, orderable] }
  client_id: { type: string }
  item: { type: string, flags: [title, searchable] }
mongo:
  collection: orders
references:
  client:
    table: client
    kind: has_one
    foreign_key: client_id
"#;

/// Clients `c1`, `c2`, each with a nested `address`; orders `o1`, `o2` (c1)
/// and `o3` (c2).
async fn setup() -> (MongoDB, MongoVistaFactory) {
    let db_name = format!("vantage_vista_contract_{}", ObjectId::new().to_hex());
    let db = MongoDB::connect(&mongo_url(), &db_name)
        .await
        .expect("connect mongo");
    db.collection::<bson::Document>("clients")
        .insert_many(vec![
            bson::doc! { "_id": "c1", "name": "Marty", "address": { "city": "Hill Valley" } },
            bson::doc! { "_id": "c2", "name": "Doc", "address": { "city": "Twin Pines" } },
        ])
        .await
        .unwrap();
    db.collection::<bson::Document>("orders")
        .insert_many(vec![
            bson::doc! { "_id": "o1", "client_id": "c1", "item": "skateboard" },
            bson::doc! { "_id": "o2", "client_id": "c1", "item": "almanac" },
            bson::doc! { "_id": "o3", "client_id": "c2", "item": "flux capacitor" },
        ])
        .await
        .unwrap();
    let specs: IndexMap<String, MongoVistaSpec> = [spec(CLIENT), spec(ORDER)]
        .into_iter()
        .map(|s| (s.name.clone(), s))
        .collect();
    let resolver: Arc<dyn SpecResolver<MongoVistaSpec>> =
        Arc::new(move |name: &str| specs.get(name).cloned());
    let factory = db.vista_factory().with_resolver(resolver);
    (db, factory)
}

async fn teardown(db: &MongoDB) {
    db.database()
        .drop()
        .await
        .unwrap_or_else(|e| eprintln!("drop contract db: {e}"));
}

fn spec(yaml: &str) -> MongoVistaSpec {
    serde_yaml_ng::from_str(yaml).expect("spec yaml")
}

/// Both sides of the join: the client's has-many and the order's has-one.
#[tokio::test]
async fn mongo_contract_advertised() {
    let (db, factory) = setup().await;
    contract::check_advertised(&factory.build_from_spec(spec(CLIENT)).unwrap()).await;
    contract::check_advertised(&factory.build_from_spec(spec(ORDER)).unwrap()).await;
    teardown(&db).await;
}

#[tokio::test]
async fn mongo_contract_narrowing() {
    let (db, factory) = setup().await;
    contract::check_narrowing(&factory.build_from_spec(spec(CLIENT)).unwrap()).await;
    teardown(&db).await;
}

/// Each test gets its own database, so the orders are disposable.
#[tokio::test]
async fn mongo_contract_writes() {
    let (db, factory) = setup().await;
    contract::check_advertised_writes(&factory.build_from_spec(spec(ORDER)).unwrap()).await;
    teardown(&db).await;
}

#[tokio::test]
async fn mongo_contract_clone() {
    let (db, factory) = setup().await;
    let orders = factory.build_from_spec(spec(ORDER)).unwrap();
    contract::check_clone(&orders, "client_id", CborValue::Text("c1".into())).await;
    teardown(&db).await;
}
