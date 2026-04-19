use bakery_model3::{Client, connect_surrealdb, surrealdb};
use dataset_ui_adapters::{TableStore, VantageTableAdapter, cursive_adapter::CursiveTableApp};
use vantage_table::any::AnyTable;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    connect_surrealdb().await?;

    let client_table = AnyTable::from_table(Client::surreal_table(surrealdb()));
    let dataset = VantageTableAdapter::new(client_table).await;
    let store = TableStore::new(dataset);
    let app = CursiveTableApp::new(store).await.map_err(|e| anyhow::anyhow!(e.to_string()))?;

    println!("Starting Bakery Model 3 — Cursive Client List (SurrealDB)");
    println!("Controls: ↑/↓ navigate, Enter to select, q/Esc to quit");
    println!("Click column headers to sort.");

    app.run().map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}
