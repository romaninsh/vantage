use bakery_model3::*;
use vantage_core::{Result, error, util::error::Context};
use vantage_csv::Csv;
use vantage_dataset::prelude::ReadableDataSet;

#[tokio::main]
async fn main() -> Result<()> {
    let csv = Csv::new("bakery_model3/data");

    let set_of_clients = Client::csv_table(csv.clone());

    println!("-[ get entity values out of any table ]------------------------------------");
    for client in set_of_clients
        .list()
        .await
        .with_context(|| error!("Failed to retrieve clients"))?
        .values()
    {
        println!("email: {}, client: {}", client.email, client.name);
    }

    println!("-[ get products ]------------------------------------");
    let products = Product::csv_table(csv.clone());

    for product in products
        .list()
        .await
        .with_context(|| error!("Failed to retrieve products"))?
        .values()
    {
        println!(
            "product: {}, calories: {}, price: {}",
            product.name, product.calories, product.price
        );
    }

    println!("-[ get bakeries ]------------------------------------");
    let bakeries = Bakery::csv_table(csv.clone());

    for bakery in bakeries
        .list()
        .await
        .with_context(|| error!("Failed to retrieve bakeries"))?
        .values()
    {
        println!(
            "bakery: {}, profit_margin: {}",
            bakery.name, bakery.profit_margin
        );
    }

    println!("-[ get orders ]------------------------------------");
    let orders = Order::csv_table(csv.clone());

    for order in orders
        .list()
        .await
        .with_context(|| error!("Failed to retrieve orders"))?
        .values()
    {
        println!(
            "order: is_deleted={}, lines={}",
            order.is_deleted, order.lines
        );
    }

    // TODO: conditions, count, relationships — coming soon
    // let paying = set_of_clients.clone()
    //     .with_condition(set_of_clients["is_paying_client"].eq(true));

    Ok(())
}
