mod product;

use product::Product;
use vantage_sql::prelude::*;

async fn list_products(table: &Table<SqliteDB, Product>) -> VantageResult<()> {
    for (id, p) in table.list().await? {
        println!("  {:<10} {:<12} {:>3} cents", id, p.name, p.price);
    }
    Ok(())
}

async fn list_table(table: &AnyTable) -> VantageResult<()> {
    let columns = table.column_names();

    // Header
    print!("  {:<12}", "id");
    for col in &columns {
        print!("{:<16}", col);
    }
    println!();

    // Rows
    for (id, record) in table.list_values().await? {
        print!("  {:<12}", id);
        for col in &columns {
            let val = record.get(col).map(|v| format!("{}", v)).unwrap_or_default();
            print!("{:<16}", val);
        }
        println!();
    }
    Ok(())
}

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

    let table = Product::table(db);

    println!("=== Products (typed) ===");
    list_products(&table).await?;

    let any = AnyTable::from_table(table);
    println!("\n=== {} (type-erased) ===", any.table_name());
    list_table(&any).await?;

    Ok(())
}
