use anyhow::Result;
use vantage::prelude::*;

use bakery_model::*;
use serde::{Deserialize, Serialize};

async fn create_bootstrap_db() -> Result<()> {
    // Connect to postgress and store client statically
    bakery_model::connect_postgres().await?;

    // Get the postgres client for batch execution
    let vantage_client = bakery_model::postgres();
    let client = vantage_client.client();
    let schema = tokio::fs::read_to_string("bakery_model/schema-pg.sql").await?;
    sqlx::raw_sql(&schema).execute(client).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    create_bootstrap_db().await?;

    println!("In this example, we will interract with 'product' and 'inventory' tables, which are joinable");

    let products = Product::table();
    for rec in products.get().await? {
        println!("{:?}", rec);
    }

    // Inventory does not have a table, so we will just create one
    #[derive(Clone, Serialize, Deserialize, Debug, Default)]
    struct Inventory {
        stock: i64,
    }
    impl Entity for Inventory {}
    let inventory: Table<Postgres, Inventory> = Table::new_with_entity("inventory", postgres())
        .with_id_column("product_id")
        .with_column("stock");

    for rec in inventory.get().await? {
        println!("{:?}", rec);
    }

    println!(
        "Records are separate now and do not make any sense! But lets create a new joined table"
    );

    #[derive(Clone, Serialize, Deserialize, Debug, Default)]
    struct ProductInventory {
        name: String,
        calories: i64,
        price: i64,
        i_stock: i64,
    }
    impl Entity for ProductInventory {}

    let product_inventory = products
        .clone()
        .with_join::<ProductInventory, _>(inventory.clone(), "id");

    for rec in product_inventory.get().await? {
        println!("{:?}", rec);
    }

    Ok(())
}
