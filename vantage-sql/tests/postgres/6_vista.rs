//! Vista integration: typed `Table<PostgresDB, _>` → `Vista` and YAML → `Vista`.
//!
//! Requires a running PostgreSQL on `postgres://vantage:vantage@localhost:5433/vantage`.
//! Each test uses a uniquely-suffixed table to avoid collisions. The cases
//! every SQL backend shares come from `vista_tests!` (`tests/common/vista.rs`);
//! the Postgres-only cases live in the child modules, by topic.

#![cfg(feature = "vista")]

use std::error::Error;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_sql::postgres::PostgresDB;
use vantage_sql::postgres::vista::PostgresVistaSpec;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{Capability, VistaFactory};

#[path = "6_vista/contained.rs"]
mod contained;
#[path = "6_vista/notify.rs"]
mod notify;
#[path = "6_vista/paging.rs"]
mod paging;

type TestResult = std::result::Result<(), Box<dyn Error>>;

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

async fn setup(suffix: &str) -> (PostgresDB, String) {
    let db = PostgresDB::connect(PG_URL).await.unwrap();
    let table_name = format!("vista_product_{}", suffix);

    sqlx::query(&format!("DROP TABLE IF EXISTS \"{}\"", table_name))
        .execute(db.pool())
        .await
        .unwrap();

    sqlx::query(&format!(
        "CREATE TABLE \"{}\" (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            price BIGINT NOT NULL,
            is_deleted BOOLEAN NOT NULL DEFAULT false
        )",
        table_name
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(&format!(
        "INSERT INTO \"{}\" VALUES \
         ('a', 'Alpha', 10, false), \
         ('b', 'Beta', 20, true), \
         ('c', 'Gamma', 30, false)",
        table_name
    ))
    .execute(db.pool())
    .await
    .unwrap();

    (db, table_name)
}

fn product_table(db: PostgresDB, name: &str) -> Table<PostgresDB, EmptyEntity> {
    Table::<PostgresDB, EmptyEntity>::new(name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted")
}

async fn product(suffix: &str) -> (PostgresDB, Table<PostgresDB, EmptyEntity>) {
    let (db, name) = setup(suffix).await;
    (db.clone(), product_table(db, &name))
}

/// Create suffixed `client` and `orders` tables and return their names.
async fn clients_orders(suffix: &str) -> (PostgresDB, String, String) {
    let db = PostgresDB::connect(PG_URL).await.unwrap();
    let client_t = format!("vista_client_{suffix}");
    let orders_t = format!("vista_orders_{suffix}");

    for t in [&client_t, &orders_t] {
        sqlx::query(&format!("DROP TABLE IF EXISTS \"{t}\""))
            .execute(db.pool())
            .await
            .unwrap();
    }
    sqlx::query(&format!(
        "CREATE TABLE \"{client_t}\" (id TEXT PRIMARY KEY, name TEXT NOT NULL)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE \"{orders_t}\" (id TEXT PRIMARY KEY, client_id TEXT NOT NULL, total BIGINT NOT NULL)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO \"{client_t}\" VALUES ('alice','Alice'),('bob','Bob')"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "INSERT INTO \"{orders_t}\" VALUES ('o1','alice',10),('o2','alice',20),('o3','bob',30)"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    (db, client_t, orders_t)
}

vista_tests!(PostgresVistaSpec, "postgres");

#[tokio::test]
async fn vista_lists_typed_postgres_as_cbor() -> TestResult {
    let (db, name) = setup("list").await;
    let table = product_table(db.clone(), &name);
    let vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), name);
    assert_eq!(vista.id_column(), Some("id"));

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);

    let alpha = rows.get("a").expect("row a");
    assert_eq!(
        alpha.get("name"),
        Some(&CborValue::Text("Alpha".to_string()))
    );
    Ok(())
}

#[tokio::test]
async fn vista_capabilities_advertise_read_write() -> TestResult {
    let (db, name) = setup("caps").await;
    let table = product_table(db.clone(), &name);
    let vista = db.vista_factory().from_table(table)?;

    let caps = vista.capabilities();
    assert!(caps.has(Capability::Insert));
    assert!(caps.has(Capability::Update));
    assert!(caps.has(Capability::Delete));
    assert!(caps.has(Capability::ConfineWrites));
    assert!(caps.has(Capability::Order));
    assert!(caps.has(Capability::Search));
    assert!(caps.has(Capability::FilterOps));
    assert!(caps.has(Capability::Window));
    assert!(caps.has(Capability::Traverse));
    assert!(caps.has(Capability::WritesReachHooks));
    assert!(!caps.has(Capability::Cursor));
    // Being writable says nothing about whether anyone installed the
    // `{table}_changed` trigger, so watchability is NOT inferred from it.
    assert!(!caps.has(Capability::Subscribe));
    Ok(())
}
