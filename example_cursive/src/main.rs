use dataset_ui_adapters::{cursive_adapter::CursiveTableApp, TableStore, VantageTableAdapter};
use bakery_model3::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to SurrealDB and get client table in a separate runtime
    let rt = tokio::runtime::Runtime::new()?;
    let client_table = rt.block_on(async {
        bakery_model3::connect_surrealdb().await?;
        Ok::<_, anyhow::Error>(Client::table())
    })?;

    // Create the dataset adapter and table store
    let dataset = VantageTableAdapter::new(client_table);
    let store = TableStore::new(dataset);

    // Create and run the Cursive app
    let app = CursiveTableApp::new(store)?;

    println!("Starting Bakery Model 3 - Cursive Client List...");
    println!("Controls: ↑/↓ navigate, Enter to select, q to quit");
    println!("Click column headers to sort!");
    println!("Real SurrealDB data using Vantage 0.3 architecture");

    app.run()?;

    Ok(())
}
