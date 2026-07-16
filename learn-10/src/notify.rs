//! Postgres `LISTEN/NOTIFY` bridge.
//!
//! Because we own the database, we don't have to poll it. A trigger (installed
//! in [`crate::db::setup`]) fires `NOTIFY product_changed` on every write; this
//! task listens on that channel and refreshes the Dio the instant it hears one.
//! No timer, no wasted queries — the cache reconciles exactly when, and only
//! when, the data actually changed.

use sqlx::PgPool;
use sqlx::postgres::PgListener;
use vantage_diorama::prelude::*;
use vantage_sql::prelude::*;

pub fn spawn(dio: Dio, pool: PgPool) {
    tokio::spawn(async move {
        if let Err(e) = run(dio, pool).await {
            e.report();
        }
    });
}

async fn run(dio: Dio, pool: PgPool) -> VantageResult<()> {
    let mut listener = PgListener::connect_with(&pool)
        .await
        .context("open pg listener")?;
    listener
        .listen("product_changed")
        .await
        .context("LISTEN product_changed")?;
    println!("listening for Postgres NOTIFY on `product_changed`");

    loop {
        listener.recv().await.context("recv notification")?;
        dio.refresh().await?;
    }
}
