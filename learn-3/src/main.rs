mod category;
mod db;
mod product;
mod vantage_axum;

use axum::Router;
use category::{Category, CategoryTable};
use vantage_axum::crud;
use vantage_mongodb::prelude::*;

#[tokio::main]
async fn main() -> VantageResult<()> {
    db::init("mongodb://localhost:27017", "learn3").await?;

    let app = Router::new()
        .nest("/categories", crud(|db, _| Category::table(db).clone()))
        .nest(
            "/categories/{cat_id}/products",
            crud(|db, p| {
                let mut c = Category::table(db).clone();
                c.add_condition(c.id().eq(p["cat_id"].as_str()));
                c.ref_products()
            }),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
