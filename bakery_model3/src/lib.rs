use anyhow::Result;
use std::sync::OnceLock;
use surreal_client::SurrealConnection;
pub use vantage_surrealdb::SurrealDB;

pub mod bakery;
pub use bakery::*;

pub mod client;
pub use client::*;

pub mod product;
pub use product::*;

pub mod order;
pub use order::*;

static SURREALDB: OnceLock<SurrealDB> = OnceLock::new();

pub fn set_surrealdb(db: SurrealDB) -> Result<()> {
    SURREALDB
        .set(db)
        .map_err(|_| anyhow::anyhow!("Failed to set SurrealDB instance"))
}

pub fn surrealdb() -> SurrealDB {
    SURREALDB
        .get()
        .expect("SurrealDB has not been initialized. use connect_surrealdb()")
        .clone()
}

pub async fn connect_surrealdb() -> Result<()> {
    let dsn = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "ws://root:root@localhost:8000/bakery/v2".to_string());

    let client = SurrealConnection::dsn(&dsn)?.connect().await?;

    let db = SurrealDB::new(client);
    set_surrealdb(db)
}
