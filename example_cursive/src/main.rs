use bakery_model3::{connect_surrealdb, surrealdb, Client};
use dataset_ui_adapters::{cursive_adapter::CursiveTableApp, TableStore, VantageTableAdapter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    connect_surrealdb().await?;

    let db = surrealdb();
    let vista = db
        .vista_factory()
        .from_table(Client::surreal_table(db.clone()))?;
    let dataset = VantageTableAdapter::new(vista).await;
    let store = TableStore::new(dataset);
    let app = CursiveTableApp::new(store)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    println!("Starting Bakery Model 3 — Cursive Client List (SurrealDB)");
    println!("Controls: ↑/↓ navigate, Enter to select, q/Esc to quit");
    println!("Click column headers to sort.");

    app.run().map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}
