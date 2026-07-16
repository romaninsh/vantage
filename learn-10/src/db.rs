//! Connection + schema — Postgres only.
//!
//! The previous chapter migrated this app off SQLite. Having committed to one
//! backend, the code drops every `#[cfg]` and uses Postgres directly — starting
//! with a trigger that `NOTIFY`s on every change to `product`, which the
//! listener in the binary's `notify` module turns into an instant cache
//! refresh.

use vantage_sql::postgres::PostgresDB;
use vantage_sql::prelude::*;

pub async fn connect() -> VantageResult<PostgresDB> {
    let url = std::env::var("DATABASE_URL").map_err(|_| {
        vantage_core::error!(
            "DATABASE_URL must be set, e.g. postgres://vantage:vantage@localhost:5433/vantage"
        )
    })?;
    PostgresDB::connect(&url).await.context("connect postgres")
}

/// Create the `product` table and its NOTIFY trigger if absent. The shelf
/// starts empty; the `mutator` binary fills it with deliveries.
pub async fn setup(db: &PostgresDB) -> VantageResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS product (
            id      TEXT PRIMARY KEY,
            name    TEXT NOT NULL,
            price   BIGINT NOT NULL,
            stock   BIGINT NOT NULL,
            created BIGINT NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .context("create table")?;

    // Announce every change on the `product_changed` channel. `FOR EACH
    // STATEMENT` fires once per statement, not per row — one notification is
    // enough to trigger a reconcile.
    sqlx::query(
        "CREATE OR REPLACE FUNCTION product_notify() RETURNS trigger AS $$
         BEGIN PERFORM pg_notify('product_changed', ''); RETURN NULL; END;
         $$ LANGUAGE plpgsql",
    )
    .execute(db.pool())
    .await
    .context("create notify function")?;
    sqlx::query("DROP TRIGGER IF EXISTS product_notify_trg ON product")
        .execute(db.pool())
        .await
        .context("drop trigger")?;
    sqlx::query(
        "CREATE TRIGGER product_notify_trg
         AFTER INSERT OR UPDATE OR DELETE ON product
         FOR EACH STATEMENT EXECUTE FUNCTION product_notify()",
    )
    .execute(db.pool())
    .await
    .context("create trigger")?;

    Ok(())
}
