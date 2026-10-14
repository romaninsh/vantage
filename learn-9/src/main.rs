mod db;
mod product;
mod sim;

use std::time::Duration;

use product::Product;
use vantage_api_adapters::axum_dio::DioRouter;
use vantage_diorama::prelude::*;
use vantage_diorama::{Cache, Loader, LoaderCx, Refresher};
use vantage_sql::prelude::*;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.report();
    }
}

async fn run() -> VantageResult<()> {
    let db = db::connect().await?;
    db::setup(&db).await?;

    // The master: the `product` table as a Vista. This line — and every line
    // below it — is the same whether `db` is SQLite or PostgreSQL.
    let master = db.vista_factory().from_table(Product::table(db.clone()))?;

    // An eager, reactive lens: load the whole table on start, reconcile it on a
    // timer. Reads come from the in-memory cache; only the reconcile touches the
    // database.
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(Loader::eager(reload))
        .refresher(Refresher::new(reload).every(Duration::from_secs(1)))
        .build()
        .context("build lens")?;
    let dio = lens.dio(master).build().await?;

    // The bar's till, writing to whichever backend is compiled in.
    sim::spawn(db.clone());

    let api = DioRouter::new(dio.clone())
        .with_column("name", "name")
        .with_column("price", "price")
        .with_column("stock", "stock")
        .with_page_size(50)
        .into_router();

    let app = axum::Router::new().nest("/api/products", api);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3009")
        .await
        .context("bind :3009")?;
    println!(
        "serving on http://localhost:3009 — \
         try: curl -N 'localhost:3009/api/products?watch=true'"
    );
    axum::serve(listener, app).await.context("server failed")
}

/// Make the cache hold exactly the master's rows.
async fn reload(cx: LoaderCx) -> VantageResult<()> {
    let rows = cx.master().list_values().await?;
    cx.reconcile(rows).await
}
