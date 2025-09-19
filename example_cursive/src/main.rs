use dataset_ui_adapters::{cursive_adapter::CursiveTableApp, TableStore, VantageTableAdapter};
use bakery_model3::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to SurrealDB and get client table
    bakery_model3::connect_surrealdb().await?;
    let client_table = Client::table();

    // Create the dataset adapter and table store
    let dataset = VantageTableAdapter::new(client_table).await;
    let store = TableStore::new(dataset);

    // Create and run the Cursive app
    let app = CursiveTableApp::new(store).await?;

    println!("Starting Bakery Model 3 - Cursive Client List...");
    println!("Controls: ↑/↓ navigate, Enter to select, q to quit");
    println!("Click column headers to sort!");
    println!("Real SurrealDB data using Vantage 0.3 architecture");

    app.run()?;

    Ok(())
}
