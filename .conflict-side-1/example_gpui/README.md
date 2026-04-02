# GPUI Table Example

A desktop application demonstrating Vantage UI Adapters with the GPUI framework, displaying real SurrealDB data.

![GPUI Example](docs/images/gpui.png)

## Overview

This example shows how to integrate [Vantage UI Adapters](https://github.com/romaninsh/vantage/tree/main/vantage-ui-adapters) with GPUI to display client data from SurrealDB in a native desktop application.

## Quick Start

```bash
# Start SurrealDB and populate with data
cd ../vantage-surrealdb
./run.sh
# In another terminal:
./ingress.sh

# Run the GPUI example
cd ../example_gpui
cargo run
```

## Code Example

```rust
use bakery_model3::*;
use dataset_ui_adapters::{gpui_adapter::GpuiTableDelegate, TableStore, VantageTableAdapter};
use gpui::*;

struct TableApp {
    table: Entity<Table<GpuiTableDelegate<VantageTableAdapter<Client>>>>,
}

impl TableApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let rt = Runtime::new().expect("Failed to create tokio runtime");

        let client_table = rt.block_on(async {
            bakery_model3::connect_surrealdb().await.expect("Failed to connect to SurrealDB");
            Client::table()
        });

        let dataset = rt.block_on(async { VantageTableAdapter::new(client_table).await });
        let store = TableStore::new(dataset);
        let delegate = GpuiTableDelegate::new(store);
        let table = cx.new(|cx| Table::new(delegate, window, cx).stripe(true).border(true));

        Self { table }
    }
}

impl Render for TableApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_4()
            .gap_4()
            .child("Bakery Model 3 - GPUI Client List")
            .child(self.table.clone())
    }
}
```

## Features

- **GPU-Accelerated Rendering**: Native GPUI performance
- **Real Database Data**: Displays actual SurrealDB client records
- **Native Desktop UI**: Platform-native window and menu integration
- **Async Data Loading**: Non-blocking data fetching through Vantage adapters

## Requirements

- SurrealDB server running on `ws://localhost:8000`
- Rust with GPUI dependencies
- Sample data populated via `vantage-surrealdb/ingress.sh`

## Integration

This example is part of the [Vantage UI Adapters](https://github.com/romaninsh/vantage/tree/main/vantage-ui-adapters) ecosystem, demonstrating how the same data layer works across different UI frameworks.
