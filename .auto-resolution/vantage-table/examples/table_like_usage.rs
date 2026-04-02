//! Example showing how to use TableLike trait for dynamic dispatch
//!
//! This demonstrates how you can work with tables without dealing with generics,
//! making it easier to store different table types in collections or pass them
//! around without complex type parameters.

use vantage_table::mocks::MockTableSource;
use vantage_table::prelude::*;

fn main() {
    // Create different table types
    let datasource1 = MockTableSource::new();
    let users_table = Table::new("users", datasource1)
        .with_column("id")
        .with_column("name")
        .with_column("email");

    let datasource2 = MockTableSource::new();
    let orders_table = Table::new("orders", datasource2)
        .with_column("order_id")
        .with_column("customer_id")
        .with_column("amount")
        .with_column("status");

    // Convert to TableLike for uniform handling
    let table_like_users: Box<dyn TableLike> = Box::new(users_table);
    let table_like_orders: Box<dyn TableLike> = Box::new(orders_table);

    // Store in a collection - this is much easier than dealing with generics!
    let tables: Vec<Box<dyn TableLike>> = vec![table_like_users, table_like_orders];

    // Process all tables uniformly
    println!("Processing {} tables:", tables.len());
    for (i, table) in tables.iter().enumerate() {
        let columns = table.columns();
        println!("\nTable {}: {} columns", i + 1, columns.len());

        for (_key, column) in columns.iter() {
            let alias_info = match column.alias() {
                Some(alias) => format!(" (alias: {})", alias),
                None => String::new(),
            };
            println!("  - {}{}", column.name(), alias_info);
        }
    }

    // You can also work with a single table without generics
    let datasource3 = MockTableSource::new();
    let products_table = Table::new("products", datasource3)
        .with_column("product_id")
        .with_column("name")
        .with_column("price");

    process_table_dynamically(Box::new(products_table));
}

/// Function that accepts any table via TableLike trait
/// This is much cleaner than having generic parameters everywhere
fn process_table_dynamically(table: Box<dyn TableLike>) {
    println!("\nProcessing table dynamically:");
    let columns = table.columns();

    println!("Found {} columns:", columns.len());
    for (_key, column) in columns.iter() {
        println!(
            "  Column: {} -> Expression: {:?}",
            column.name(),
            column.expr()
        );
    }
}
