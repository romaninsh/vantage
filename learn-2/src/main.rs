mod category;
mod product;

use category::Category;
use product::{Product, ProductTable};
use vantage_sql::prelude::*;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.report();
    }
}

async fn run() -> VantageResult<()> {
    let db = SqliteDB::connect("sqlite:products.db")
        .await
        .context("Failed to connect to products.db")?;

    let filter = std::env::args().nth(1);

    let products = match &filter {
        Some(search) => Category::table(db.clone())
            .with_search(search)
            .get_ref_as::<Product>("products")?,
        None => Product::table(db),
    };

    products.print().await?;

    Ok(())
}
