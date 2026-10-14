//! Vista integration: typed `Table<SurrealDB, _>` → `Vista` and YAML → `Vista`.
//!
//! Requires a running SurrealDB. Set `SURREALDB_URL` (defaults to
//! `cbor://root:root@localhost:8000/bakery/v2`). Each test runs in a fresh
//! randomised database so parallel runs don't trigger transaction conflicts.
//!
//! - this file: the connection, the product table and its seed;
//! - `reads`: listing, lookup by id, YAML specs and capabilities;
//! - `writes`: insert, delete and upsert round trips;
//! - `narrow`: conditions, order and search;
//! - `windows`: positional windows, and the refused cursor;
//! - `query_sourced`: `rhai:` sources, `modify:` scripts and `base:` vistas.

#![cfg(feature = "vista")]

mod narrow;
#[cfg(feature = "rhai")]
mod query_sourced;
mod reads;
mod windows;
mod writes;

use std::error::Error;

use ciborium::Value as CborValue;
use surreal_client::SurrealConnection;
use uuid::Uuid;
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

type TestResult = std::result::Result<(), Box<dyn Error>>;

/// Build a per-test DSN with an isolated database name. The base DSN comes
/// from `SURREALDB_URL` (root auth + host); each test substitutes its own
/// database to avoid transaction conflicts between parallel runs.
fn surreal_dsn(database: &str) -> String {
    let base = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".to_string());
    let (prefix, _) = base.rsplit_once('/').unwrap_or((&base, ""));
    format!("{}/{}", prefix, database)
}

async fn setup() -> (SurrealDB, String) {
    let database = format!("vista_{}", Uuid::new_v4().simple());
    let client = SurrealConnection::dsn(surreal_dsn(&database))
        .expect("Invalid SURREALDB_URL DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    let db = SurrealDB::new(client);
    let table = format!("product_{}", Uuid::new_v4().simple());
    (db, table)
}

fn product_table(db: SurrealDB, table_name: &str) -> Table<SurrealDB, EmptyEntity> {
    Table::<SurrealDB, EmptyEntity>::new(table_name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted")
}

async fn seed_row(
    db: &SurrealDB,
    table_name: &str,
    id: &str,
    name: &str,
    price: i64,
    is_deleted: bool,
) -> std::result::Result<Thing, Box<dyn Error>> {
    let record = format!("{}:{}", table_name, id);
    let name_owned = name.to_string();
    db.execute(&surreal_expr!(
        &format!(
            "CREATE {} SET name = {{}}, price = {{}}, is_deleted = {{}}",
            record
        ),
        name_owned,
        price,
        is_deleted
    ))
    .await?;
    Ok(Thing::new(table_name, id))
}

/// A row's `name` as text, empty when it isn't one.
fn name_of(row: &Record<CborValue>) -> String {
    match row.get("name") {
        Some(CborValue::Text(s)) => s.clone(),
        _ => String::new(),
    }
}
