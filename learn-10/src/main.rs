use std::sync::Arc;

use learn_10::db;
use learn_10::product::Product;
use tower_http::services::ServeDir;
use vantage_api_adapters::axum_dio::DioRouter;
use vantage_diorama::prelude::*;
use vantage_sql::prelude::*;
use vantage_vista::SortDirection;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.report();
    }
}

async fn run() -> VantageResult<()> {
    let db = db::connect().await?;
    db::setup(&db).await?;

    // Order the shelf by creation time, so a drink keeps its place as it sells
    // and new deliveries append at the end.
    //
    // `with_notify(true)` is us telling the factory that `db::setup` installed
    // the `product_changed` trigger. Postgres can't be asked whether a trigger
    // exists, and listening on a channel nobody feeds blocks forever — so the
    // application declares it, and that declaration is what lets the Vista
    // advertise `can_watch` below.
    let mut master = db
        .vista_factory()
        .with_notify(true)
        .from_table(Product::table(db.clone()))?;
    master.add_order("created", SortDirection::Ascending)?;

    // Eager cache, and no refresh timer at all: the NOTIFY listener refreshes
    // the instant a write lands, never on a poll.
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .on_refresh(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().clear().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .build()
            .context("build lens")?,
    );
    let dio = lens.make_dio(master).await?;

    // Refresh the moment Postgres says the table changed. `dio.watch()` sees the
    // master Vista advertises `can_watch` and subscribes over `LISTEN/NOTIFY`
    // (on the `product_changed` channel our trigger feeds) — no hand-written
    // listener. Nothing in *this* process ever writes to `product`; the separate
    // `mutator` binary does, so whatever you see arrived over the database. This
    // is the exact same call the SurrealDB chapter makes — the backend-agnostic
    // payoff of the watch capability.
    dio.watch().await?;

    let api = DioRouter::new(dio.clone())
        .with_column("id", "id")
        .with_column("name", "name")
        .with_column("price", "price")
        .with_column("stock", "stock")
        // Identity-keyed watch: the stream reports a sold-out drink as a
        // `DELETED` event (by id), so the frontend can animate its removal.
        .key_by("id")
        .with_page_size(50)
        .into_router();

    // The API, plus the static React frontend served from `frontend/`.
    let frontend = concat!(env!("CARGO_MANIFEST_DIR"), "/frontend");
    let app = axum::Router::new()
        .nest("/api/products", api)
        .fallback_service(ServeDir::new(frontend));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3010")
        .await
        .context("bind :3010")?;
    println!("serving on http://localhost:3010  (run the mutator to fill the shelf)");
    axum::serve(listener, app).await.context("server failed")
}
