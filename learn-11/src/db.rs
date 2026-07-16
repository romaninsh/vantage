//! Connection + schema — SurrealDB.
//!
//! The Postgres chapter needed a hand-written trigger + `pg_notify` function to
//! turn a write into a change event. SurrealDB streams changes natively over
//! the WebSocket via `LIVE SELECT`, so there is nothing to wire: setup is just
//! making sure the table exists. The live subscription is started later, in the
//! server, with a single `dio.watch()`.

use surreal_client::SurrealConnection;
use vantage_core::{Result, error};
use vantage_surrealdb::surrealdb::SurrealDB;

/// Connect to SurrealDB and ensure the `product` table exists.
///
/// Reads `SURREALDB_URL` (defaults to a local instance). Start one with
/// `vantage-surrealdb/scripts/start.sh` (docker) or
/// `surreal start --user root --pass root memory`.
pub async fn connect() -> Result<SurrealDB> {
    let dsn = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bar/v1".to_string());

    let client = SurrealConnection::dsn(&dsn)
        .map_err(|e| error!("invalid SURREALDB_URL DSN", details = e.to_string()))?
        .connect()
        .await
        .map_err(|e| error!("connect surrealdb", dsn = &dsn, details = e.to_string()))?;

    // The one bit of schema: a table for `LIVE SELECT` to attach to. No trigger,
    // no stored function — SurrealDB emits change frames on its own.
    client
        .query("DEFINE TABLE IF NOT EXISTS product SCHEMALESS", None)
        .await
        .map_err(|e| error!("define product table", details = e.to_string()))?;

    Ok(SurrealDB::new(client))
}
