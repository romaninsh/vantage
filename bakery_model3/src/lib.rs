pub use vantage_csv::{AnyCsvType, Csv, CsvType};
pub use vantage_mongodb::{AnyMongoType, MongoDB, MongoType};
pub use vantage_sql::postgres::{AnyPostgresType, PostgresDB, PostgresType};
pub use vantage_sql::sqlite::{AnySqliteType, SqliteDB, SqliteType};

pub mod animal;
pub mod bakery;
pub mod client;
pub mod order;
pub mod product;

pub use animal::*;
pub use bakery::*;
pub use client::*;
pub use order::*;
pub use product::*;

use std::sync::OnceLock;
pub use surreal_client::SurrealConnection;
pub use vantage_surrealdb::surrealdb::SurrealDB;

static SURREALDB: OnceLock<SurrealDB> = OnceLock::new();

pub fn set_surrealdb(db: SurrealDB) -> vantage_core::Result<()> {
    SURREALDB
        .set(db)
        .map_err(|_| vantage_core::error!("SurrealDB instance already set"))
}

pub fn surrealdb() -> SurrealDB {
    SURREALDB
        .get()
        .expect("SurrealDB not initialized — call connect_surrealdb() first")
        .clone()
}

pub async fn connect_surrealdb() -> vantage_core::Result<()> {
    connect_surrealdb_with_debug(false).await
}

pub async fn connect_surrealdb_with_debug(debug: bool) -> vantage_core::Result<()> {
    let dsn = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".to_string());

    let client = SurrealConnection::dsn(&dsn)
        .map_err(|e| vantage_core::error!("Invalid DSN", dsn = &dsn, details = e.to_string()))?
        .with_debug(debug)
        .connect()
        .await
        .map_err(|e| {
            vantage_core::error!(
                "Failed to connect to SurrealDB",
                dsn = &dsn,
                details = e.to_string()
            )
        })?;

    set_surrealdb(SurrealDB::new(client))?;

    if debug {
        println!("🔧 Debug mode enabled — queries will be logged");
    }

    Ok(())
}
