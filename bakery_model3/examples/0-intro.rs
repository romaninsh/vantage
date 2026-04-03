use bakery_model3::*;
use vantage_core::{Result, error, util::error::Context};
use vantage_csv::{AnyCsvType, Csv};
use vantage_dataset::prelude::ReadableDataSet;
use vantage_table::operation::Operation;

#[tokio::main]
async fn main() -> Result<()> {
    let csv = Csv::new("bakery_model3/data");

    let set_of_clients = Client::csv_table(csv.clone());

    println!("-[ all clients ]------------------------------------");
    for client in set_of_clients
        .list()
        .await
        .with_context(|| error!("Failed to retrieve clients"))?
        .values()
    {
        println!(
            "  {} ({}) - paying: {}",
            client.name, client.email, client.is_paying_client
        );
    }

    println!("\n-[ paying clients only (condition) ]------------------------------------");
    let mut paying_clients = Client::csv_table(csv.clone());
    paying_clients.add_condition(paying_clients["is_paying_client"].eq(AnyCsvType::new(true)));

    for client in paying_clients
        .list()
        .await
        .with_context(|| error!("Failed to retrieve paying clients"))?
        .values()
    {
        println!("  {} ({})", client.name, client.email);
    }

    println!("\n-[ orders for paying clients (traversal) ]------------------------------------");
    let orders_for_paying = paying_clients.get_ref_as::<Csv, Order>("orders")?;
    for order in orders_for_paying
        .list()
        .await
        .with_context(|| error!("Failed to retrieve orders"))?
        .values()
    {
        println!(
            "  client_id={:?}, is_deleted={}, lines={:?}",
            order.client_id, order.is_deleted, order.lines
        );
    }

    println!("\n-[ all orders ]------------------------------------");
    let orders = Order::csv_table(csv.clone());
    for order in orders
        .list()
        .await
        .with_context(|| error!("Failed to retrieve orders"))?
        .values()
    {
        println!(
            "  client_id={:?}, is_deleted={}",
            order.client_id, order.is_deleted
        );
    }

    Ok(())
}
