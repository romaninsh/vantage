use anyhow::Result;
use vantage::{dataset::WritableDataSet, prelude::*};

use bakery_model::*;

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

#[tokio::test]
async fn test_abc() {
    panic!("aoeu");
}

#[tokio::main]
async fn main() -> Result<()> {
    create_bootstrap_db().await?;

    println!("In this example, we will be interracting with the records and testing conditions");
    let products = Product::table();

    println!(
        "We are starting with {} products",
        products.count().get_one_untyped().await?
    );

    println!("");
    println!("Adding a single new product");
    let nuclear_sandwich_id = products
        .insert(Product {
            name: "Nuclear Sandwich".to_string(),
            calories: 100,
            bakery_id: 1,
            price: 110,
        })
        .await?
        .unwrap();

    println!(
        "After adding \"Nuclear Sandwich\" (id={}) we are left with {} products",
        &nuclear_sandwich_id,
        products.count().get_one_untyped().await?
    );

    // So far we didn't know about the soft delete field, but lets add it now
    let product_sd = products
        .clone()
        .with_extension(SoftDelete::new("is_deleted"));

    // Next we are going to delete a nuclear sandwitch with "sd"
    product_sd
        .clone()
        .with_id(nuclear_sandwich_id)
        .delete()
        .await?;

    println!(
        "After soft-deleting \"Nuclear Sandwich\" we are left with {} SD (soft delete) products",
        product_sd.count().get_one_untyped().await?
    );
    println!(
        "However as per our old set (no SD) we still have {} products",
        products.count().get_one_untyped().await?
    );

    Ok(())
}
