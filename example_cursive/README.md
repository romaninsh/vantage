# Cursive Table Example

A terminal application demonstrating Vantage UI Adapters with the Cursive framework, displaying real SurrealDB data.

![Cursive Example](docs/images/cursive.png)

## Overview

This example shows how to integrate [Vantage UI Adapters](https://github.com/romaninsh/vantage/tree/main/vantage-ui-adapters) with Cursive to display client data from SurrealDB in an interactive terminal user interface.

## Quick Start

```bash
# Start SurrealDB and populate with data
cd ../vantage-surrealdb
./run.sh
# In another terminal:
./ingress.sh

# Run the Cursive example
cd ../example_cursive
cargo run
```

## Code Example

```rust
use bakery_model3::*;
use dataset_ui_adapters::{cursive_adapter::CursiveTableApp, TableStore, VantageTableAdapter};

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
    app.run()?;

    Ok(())
}
```

## Controls

- **↑/↓ Arrow Keys**: Navigate up/down through table rows
- **Enter**: Select row (opens dialog with row details)
- **q**: Quit the application
- **Mouse Click**: Click column headers to sort by that column

## Features

- **Interactive Terminal UI**: Rich terminal interface with mouse support
- **Real Database Data**: Displays actual SurrealDB client records
- **Sortable Columns**: Click column headers to sort data
- **Row Selection**: Select rows to view detailed information
- **Async Data Loading**: Non-blocking data fetching through Vantage adapters

## Requirements

- SurrealDB server running on `ws://localhost:8000`
- Rust with Cursive dependencies
- Terminal with mouse support (optional)
- Sample data populated via `vantage-surrealdb/ingress.sh`

## Integration

This example is part of the [Vantage UI Adapters](https://github.com/romaninsh/vantage/tree/main/vantage-ui-adapters) ecosystem, demonstrating how the same data layer works across different UI frameworks.
