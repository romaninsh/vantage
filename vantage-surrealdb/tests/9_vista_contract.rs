//! `vantage_vista::contract` over a live SurrealDB.
//!
//! Each test runs in a fresh randomised database. Reads `SURREALDB_URL`
//! (defaults to localhost:8000) for the host.

#![cfg(feature = "vista")]

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use surreal_client::SurrealConnection;
use uuid::Uuid;
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::vista::{SurrealSpecResolver, SurrealVistaFactory, SurrealVistaSpec};
use vantage_vista::{VistaFactory, contract};

fn surreal_dsn(database: &str) -> String {
    let base = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".to_string());
    let (prefix, _) = base.rsplit_once('/').unwrap_or((&base, ""));
    format!("{}/{}", prefix, database)
}

const BAKERY: &str = r#"
name: bakery
columns:
  id: { type: thing, flags: [id, orderable] }
  name: { type: string, flags: [title, searchable] }
surreal:
  table: bakery
references:
  clients:
    table: client
    kind: has_many
    foreign_key: bakery
"#;

const CLIENT: &str = r#"
name: client
columns:
  id: { type: thing, flags: [id, orderable] }
  name: { type: string, flags: [title, searchable] }
  city: { type: string }
  bakery: { type: thing }
surreal:
  table: client
"#;

fn spec(yaml: &str) -> SurrealVistaSpec {
    serde_yaml_ng::from_str(yaml).expect("spec yaml")
}

/// One bakery with clients `marty`, `doc` (city `hv`) and `biff` (city `tp`).
async fn setup() -> SurrealVistaFactory {
    let database = format!("vista_contract_{}", Uuid::new_v4().simple());
    let client = SurrealConnection::dsn(surreal_dsn(&database))
        .expect("Invalid SURREALDB_URL DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    let db = SurrealDB::new(client);
    for sql in [
        "CREATE bakery:hill_valley SET name = 'Hill Valley Bakery'",
        "CREATE client:marty SET name = 'Marty', city = 'hv', bakery = bakery:hill_valley",
        "CREATE client:doc SET name = 'Doc', city = 'hv', bakery = bakery:hill_valley",
        "CREATE client:biff SET name = 'Biff', city = 'tp', bakery = bakery:hill_valley",
    ] {
        db.execute(&surreal_expr!(sql)).await.unwrap();
    }
    let specs: IndexMap<String, SurrealVistaSpec> = [
        ("bakery".into(), spec(BAKERY)),
        ("client".into(), spec(CLIENT)),
    ]
    .into_iter()
    .collect();
    let resolver: SurrealSpecResolver = Arc::new(move |name: &str| specs.get(name).cloned());
    SurrealVistaFactory::new(db).with_resolver(resolver)
}

#[tokio::test]
async fn surreal_contract_advertised() {
    let factory = setup().await;
    contract::check_advertised(&factory.build_from_spec(spec(BAKERY)).unwrap()).await;
}

/// The shell can't be cloned, so `check_advertised` skips these probes.
#[tokio::test]
async fn surreal_contract_narrowing() {
    let factory = setup().await;
    contract::check_narrowing(&mut factory.build_from_spec(spec(CLIENT)).unwrap()).await;
}

#[tokio::test]
async fn surreal_contract_writes() {
    let factory = setup().await;
    contract::check_advertised_writes(&factory.build_from_spec(spec(CLIENT)).unwrap()).await;
}

#[tokio::test]
#[ignore = "v2 phase 1: shell not cloneable"]
async fn surreal_contract_clone() {
    let clients = setup().await.build_from_spec(spec(CLIENT)).unwrap();
    contract::check_clone(&clients, "city", CborValue::Text("hv".into())).await;
}
