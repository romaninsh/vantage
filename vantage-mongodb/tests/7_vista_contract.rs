//! `vantage_vista::contract` over a live MongoDB. Set `MONGODB_URL`
//! (defaults to `mongodb://localhost:27017`). Each test runs against a fresh
//! randomised database and drops it on the way out.

#![cfg(feature = "vista")]

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bson::oid::ObjectId;
use bson::{Bson, DateTime as BsonDateTime, Document, doc};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use ciborium::Value as CborValue;
use futures_util::FutureExt;
use indexmap::IndexMap;
use vantage_core::error;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_mongodb::{MongoDB, MongoVistaFactory};
use vantage_types::temporal;
use vantage_vista::contract::{Foreign, Precision, Stored, TemporalFixture};
use vantage_vista::{SpecResolver, Vista, VistaFactory, contract};

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

fn temporal_spec(collection: &str) -> MongoVistaSpec {
    spec(&format!(
        r#"
name: event
columns:
  _id: {{ type: string, flags: [id, orderable] }}
  d: {{ type: date, flags: [orderable] }}
  t: {{ type: time, flags: [orderable] }}
  ts: {{ type: timestamp, flags: [orderable] }}
mongo:
  collection: {collection}
"#
    ))
}

/// Each `fresh()` opens a new collection; `stored` and `seed_foreign` reach
/// the last one with the driver.
struct Temporal {
    db: MongoDB,
    next: AtomicUsize,
    collection: Mutex<String>,
}

impl Temporal {
    fn new(db: MongoDB) -> Self {
        Self {
            db,
            next: AtomicUsize::new(0),
            collection: Mutex::new(String::new()),
        }
    }

    fn collection(&self) -> mongodb::Collection<Document> {
        self.db.doc_collection(&self.collection.lock().unwrap())
    }
}

#[async_trait]
impl TemporalFixture for Temporal {
    async fn fresh(&self) -> Vista {
        let name = format!("events_{}", self.next.fetch_add(1, Ordering::Relaxed));
        *self.collection.lock().unwrap() = name.clone();
        self.db
            .vista_factory()
            .build_from_spec(temporal_spec(&name))
            .expect("temporal vista")
    }

    fn precision(&self) -> Precision {
        Precision::Millis
    }

    async fn stored(&self, _vista: &Vista, id: &str, column: &str) -> vantage_core::Result<Stored> {
        let row = self
            .collection()
            .find_one(doc! { "_id": id })
            .await
            .map_err(|e| error!(format!("find {id}: {e}")))?
            .ok_or_else(|| error!(format!("{id} is not stored")))?;
        match row.get(column) {
            Some(Bson::DateTime(at)) => {
                let at = DateTime::<Utc>::from_timestamp_millis(at.timestamp_millis())
                    .ok_or_else(|| error!(format!("{column} of {id} is out of chrono's range")))?;
                let canonical = if column == "d" {
                    temporal::format_date(&at.date_naive())
                } else {
                    temporal::format_instant(&at)
                };
                Ok(Stored::Native {
                    kind: "date".into(),
                    canonical,
                })
            }
            Some(Bson::String(s)) => Ok(Stored::Text(s.clone())),
            other => Err(error!(format!("{column} of {id} is stored as {other:?}"))),
        }
    }

    async fn seed_foreign(&self, _vista: &Vista) -> vantage_core::Result<Vec<Foreign>> {
        let moment = DateTime::parse_from_rfc3339("2026-11-19T16:01:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let day = NaiveDate::from_ymd_opt(2026, 11, 19).unwrap();
        let midnight = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
        let minute = NaiveTime::from_hms_opt(16, 1, 0).unwrap();
        let bson_at =
            |at: &DateTime<Utc>| Bson::DateTime(BsonDateTime::from_millis(at.timestamp_millis()));
        let foreign: Vec<(&'static str, Bson, CborValue)> = vec![
            ("ts", bson_at(&moment), temporal::instant(&moment)),
            (
                "ts",
                Bson::String("2026-11-19T16:01:00Z".into()),
                temporal::instant(&moment),
            ),
            ("d", bson_at(&midnight), temporal::date(&day)),
            ("d", Bson::String("2026-11-19".into()), temporal::date(&day)),
            ("t", Bson::String("16:01".into()), temporal::time(&minute)),
        ];
        let docs: Vec<Document> = foreign
            .iter()
            .enumerate()
            .map(|(i, (column, written, _))| {
                let mut row = Document::new();
                row.insert("_id", format!("f{i}"));
                row.insert(*column, written.clone());
                row
            })
            .collect();
        self.collection()
            .insert_many(docs)
            .await
            .map_err(|e| error!(format!("seed foreign rows: {e}")))?;
        Ok(foreign
            .into_iter()
            .map(|(column, written, reads_as)| {
                Foreign::new(column, format!("{written:?}"), reads_as)
            })
            .collect())
    }
}

#[tokio::test]
async fn mongodb_temporal_contract() {
    let db_name = format!("vantage_vista_temporal_{}", ObjectId::new().to_hex());
    let db = MongoDB::connect(&mongo_url(), &db_name)
        .await
        .expect("connect mongo");
    let fixture = Temporal::new(db.clone());
    let outcome = AssertUnwindSafe(contract::check_temporal(&fixture))
        .catch_unwind()
        .await;
    teardown(&db).await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
