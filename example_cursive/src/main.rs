use dataset_ui_adapters::{cursive_adapter::CursiveTableApp, MockProductDataSet, TableStore};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create the dataset and table store
    let dataset = MockProductDataSet::new();
    let store = TableStore::new(dataset);

    // Create and run the Cursive app
    let app = CursiveTableApp::new(store)?;

    println!("Starting Cursive Table Example...");
    println!("Controls: ↑/↓ navigate, Enter to select, q to quit");
    println!("Click column headers to sort!");

    app.run()?;

    Ok(())
}
