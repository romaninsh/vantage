//! The reactive server — SurrealDB.
//!
//! Identical in shape to the Postgres chapter's server, with two lines changed:
//! the Vista comes from `SurrealVistaFactory` instead of `db.vista_factory()`,
//! and the live feed is a single transparent `live()` instead of a
//! hand-wired NOTIFY listener. The Dio, Lens, `DioRouter`, and React frontend
//! are byte-for-byte the same reactive stack — the whole point of the exercise.

use std::process::Termination;

use learn_11::db;
use learn_11::product::Product;
use tower_http::services::ServeDir;
use vantage_api_adapters::axum_dio::DioRouter;
use vantage_core::{Context, Result};
use vantage_dataset::prelude::*;
use vantage_diorama::prelude::*;
use vantage_surrealdb::vista::factory::SurrealVistaFactory;
use vantage_vista::SortDirection;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.report();
    }
}

async fn run() -> Result<()> {
    let db = db::connect().await?;

    // Order the shelf by creation time, so a drink keeps its place as it sells
    // and new deliveries append at the end.
    let mut master =
        SurrealVistaFactory::new(db.clone()).from_table(Product::surreal_table(db.clone()))?;
    master.set_order(Some(("created", SortDirection::Ascending)))?;

    // Eager cache, no refresh timer: the live follow applies each change the
    // instant it lands. The eager start still matters after that: a refresh
    // re-runs it, and the follow refreshes if the live subscription ever lags
    // or drops.
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(Loader::eager(|cx: LoaderCx| async move {
            let rows = cx.master().list_values().await?;
            cx.reconcile(rows).await
        }))
        .build()
        .context("build lens")?;

    // The transparent live feed. Because the master Vista advertises
    // `can_watch`, `live()` subscribes to SurrealDB `LIVE SELECT` and applies
    // each CREATE/UPDATE/DELETE to the cache as it happens — no polling, no
    // backend-specific code here. Nothing in *this* process writes to `product`;
    // the separate `mutator` binary does, so whatever you see arrived over the
    // database.
    let dio = lens.dio(master).live().build().await?;

    let api = DioRouter::new(dio.clone())
        .with_column("id", "id")
        .with_column("name", "name")
        .with_column("price", "price")
        .with_column("stock", "stock")
        // Identity-keyed watch: a sold-out drink is reported as a `DELETED`
        // event (by id), so the frontend can animate its removal.
        .key_by("id")
        .with_page_size(50)
        .into_router();

    // The API, plus the static React frontend served from `frontend/`.
    let frontend = concat!(env!("CARGO_MANIFEST_DIR"), "/frontend");
    let app = axum::Router::new()
        .nest("/api/products", api)
        .fallback_service(ServeDir::new(frontend));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3011")
        .await
        .context("bind :3011")?;
    println!("serving on http://localhost:3011  (run the mutator to fill the shelf)");
    axum::serve(listener, app).await.context("server failed")
}
