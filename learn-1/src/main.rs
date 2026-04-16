use vantage_sql::prelude::*;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.report();
    }
}

async fn run() -> VantageResult<()> {
    // Connect to SQLite
    let db = SqliteDB::connect("sqlite:products.db?mode=ro")
        .await
        .context("Failed to connect to products.db")?;

    // 1. Basic SELECT
    let select = SqliteSelect::new()
        .with_source("product")
        .with_field("name")
        .with_field("price");

    println!("=== Query preview ===");
    println!("{}", select.preview());

    // 2. Execute and print raw result
    let result = db.execute(&select.expr()).await?;
    let json: serde_json::Value = result.into();
    println!("\n=== Raw result ===");
    println!("{:#}", json);

    // 3. Add a condition with sqlite_expr!
    let select = SqliteSelect::new()
        .with_source("product")
        .with_field("name")
        .with_field("price")
        .with_condition(sqlite_expr!("\"is_deleted\" = {}", false));

    println!("\n=== With condition ===");
    println!("{}", select.preview());

    // 4. Typed columns and operators
    let is_deleted = Column::<bool>::new("is_deleted");
    let price = Column::<i64>::new("price");

    let select = SqliteSelect::new()
        .with_source("product")
        .with_field("name")
        .with_field("price")
        .with_condition(is_deleted.eq(false))
        .with_condition(price.gt(150));

    println!("\n=== Typed columns ===");
    println!("{}", select.preview());

    let result = db.execute(&select.expr()).await?;
    let json: serde_json::Value = result.into();
    println!("{:#}", json);

    // 5. Aggregates
    let all_products = SqliteSelect::new()
        .with_source("product")
        .with_condition(Column::<bool>::new("is_deleted").eq(false));

    let count = db
        .aggregate(&all_products, "count", Column::<AnySqliteType>::new("id"))
        .await?;
    println!("\n=== Aggregates ===");
    println!("Active products: {}", count);

    let sum = db
        .aggregate(&all_products, "sum", Column::<AnySqliteType>::new("price"))
        .await?;
    println!("Total price: {} cents", sum);

    Ok(())
}
