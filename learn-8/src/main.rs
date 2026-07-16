mod product;
mod sim;

use std::sync::Arc;
use std::time::Duration;

use product::Product;
use vantage_api_adapters::axum_dio::DioRouter;
use vantage_diorama::prelude::*;
use vantage_sql::prelude::*;

/// A file-backed database, so the shelf survives a restart — and so you can
/// poke it from another terminal with the `sqlite3` CLI and watch the change
/// stream out. It lives beside the crate (not the working directory), and
/// `mode=rwc` creates it on first run.
const DB_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/products.db");

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.report();
    }
}

async fn run() -> VantageResult<()> {
    let url = format!("sqlite:{DB_PATH}?mode=rwc");
    let db = SqliteDB::connect(&url).await.context("connect sqlite")?;
    setup(&db).await?;

    // The master: the SQLite `product` table as a Vista. Unlike the S3 path,
    // this backend already sorts, searches, and paginates — there is no
    // capability to inject, only a live local copy to keep.
    let master = db.vista_factory().from_table(Product::table(db.clone()))?;

    // An eager, reactive lens. `on_start` loads the whole table into an
    // in-memory cache before the server answers a single request; every second
    // `on_refresh` rebuilds it from the master, so a sale the till commits — or
    // an edit you make with `sqlite3` — appears on the next tick. Reads never
    // touch SQLite; only the reconcile does.
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
            .refresh_every(Duration::from_secs(1))
            .build()
            .context("build lens")?,
    );
    let dio = lens.make_dio(master).await?;

    // The bar's till: a background task selling and restocking, so the data
    // actually moves while a watch is open.
    sim::spawn(db.clone());

    // The whole API surface: GET + watch on the listing and on each product.
    // The columns double as the watch sceneries' demand.
    let api = DioRouter::new(dio.clone())
        .with_column("name", "name")
        .with_column("price", "price")
        .with_column("stock", "stock")
        .with_page_size(50)
        .into_router();

    let app = axum::Router::new().nest("/api/products", api);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3008")
        .await
        .context("bind :3008")?;
    println!(
        "serving on http://localhost:3008 — \
         try: curl -N 'localhost:3008/api/products?watch=true'"
    );
    axum::serve(listener, app).await.context("server failed")
}

/// Create the `product` table if absent and stock the shelf on first run.
async fn setup(db: &SqliteDB) -> VantageResult<()> {
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
