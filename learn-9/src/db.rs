//! Backend selection — the *only* file that names a concrete database.
//!
//! SQLite by default; PostgreSQL under `--features pg`, and only the selected
//! backend is compiled in. The rest of the app is written against the [`Db`]
//! alias, so moving from a file on disk to a Postgres server is a compile flag,
//! not a rewrite.

use vantage_sql::prelude::*;

/// The selected datasource. SQLite by default; PostgreSQL under `--features pg`.
#[cfg(not(feature = "pg"))]
pub type Db = vantage_sql::sqlite::SqliteDB;
#[cfg(feature = "pg")]
pub type Db = vantage_sql::postgres::PostgresDB;

/// Open the database. SQLite takes a file beside the crate (`mode=rwc` creates
/// it); PostgreSQL reads its URL from `DATABASE_URL`.
pub async fn connect() -> VantageResult<Db> {
    #[cfg(not(feature = "pg"))]
    let url = format!(
        "sqlite:{}?mode=rwc",
        concat!(env!("CARGO_MANIFEST_DIR"), "/products.db")
    );
    #[cfg(feature = "pg")]
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| vantage_core::error!("DATABASE_URL must be set for the `pg` build"))?;

    Db::connect(&url).await.context("connect db")
}

/// Create the `product` table if absent and stock the shelf on first run. The
/// DDL is plain SQL both backends accept (`BIGINT` is SQLite's INTEGER affinity
/// and Postgres's 64-bit integer), so one definition serves both.
pub async fn setup(db: &Db) -> VantageResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS product (
            id    TEXT PRIMARY KEY,
            name  TEXT NOT NULL,
            price BIGINT NOT NULL,
            stock BIGINT NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .context("create table")?;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM product")
        .fetch_one(db.pool())
        .await
        .context("count products")?;
    if count == 0 {
        for (id, name, price, stock) in [
            ("p1", "Espresso", 280_i64, 12_i64),
            ("p2", "Cappuccino", 340, 8),
            ("p3", "Cold Brew", 420, 5),
            ("p4", "Croissant", 260, 6),
            ("p5", "Cheesecake", 520, 3),
        ] {
            sqlx::query("INSERT INTO product (id, name, price, stock) VALUES ($1, $2, $3, $4)")
                .bind(id)
                .bind(name)
                .bind(price)
                .bind(stock)
                .execute(db.pool())
                .await
                .context("seed product")?;
        }
    }
    Ok(())
}
