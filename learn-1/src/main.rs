use vantage_sql::prelude::*;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.report();
    }
}

async fn run() -> VantageResult<()> {
    // Connect to our SQLite database
    let db = SqliteDB::connect("sqlite:products.db?mode=ro")
        .await
        .context("Failed to connect to products.db")?;

    // Build a condition to exclude soft-deleted records
    let price = Column::<i64>::new("price");
    let is_deleted = Column::<bool>::new("is_deleted");

    // Build a SELECT query with conditions
    let select = SqliteSelect::new()
        .with_source("product")
        .with_field("name")
        .with_field("price")
        .with_typed_condition(is_deleted.eq(false))
        .with_typed_condition(price.gt(150));

    println!("Query: {}\n", select.preview());

    // Execute and print raw result
    let result = db.execute(&select.expr()).await?;
    println!("{:?}", result);

    Ok(())
}
