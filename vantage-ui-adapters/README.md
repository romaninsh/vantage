# Dataset UI Adapters

A Rust crate providing universal table data adapters for major UI frameworks. This crate implements a layered architecture that bridges between your data layer and various UI framework table components, with intelligent caching and async data loading.

## Architecture

```
┌─────────────────┐    ┌──────────────┐    ┌─────────────────────┐
│   Your DataSet  │───▶│ TableStore   │───▶│ Framework Adapters  │
│   (Dry/Remote)  │    │ (Caching)    │    │ (egui, GPUI, etc.)  │
└─────────────────┘    └──────────────┘    └─────────────────────┘
```

- **DataSet**: Your existing dry dataset that can query but never caches
- **TableStore**: Intermediate caching layer that handles smart querying and data storage
- **Framework Adapters**: UI framework-specific implementations

## Supported UI Frameworks

- **egui** - Immediate mode GUI with `egui-data-table` integration
- **GPUI** - GPU-accelerated UI framework (from Zed team)
- **Slint** - Declarative UI toolkit with native performance
- **Tauri** - Web-based desktop apps with Rust backend
- **Ratatui** - Terminal-based UI framework for modern TUI applications
- **Cursive** - Terminal UI framework with `cursive_table_view` integration

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
dataset-ui-adapters = { path = ".", features = ["egui"] }
# Or for terminal UI:
# dataset-ui-adapters = { path = ".", features = ["ratatui"] }
# dataset-ui-adapters = { path = ".", features = ["cursive"] }
```

Basic usage:

```rust
use dataset_ui_adapters::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create your dataset (implements DataSet trait)
    let dataset = MockProductDataSet::new();

    // 2. Wrap in TableStore for caching
    let store = TableStore::new(dataset).with_page_size(100);

    // 3. Create framework-specific adapter
    #[cfg(feature = "egui")]
    {
        let mut table = egui_adapter::EguiTable::new(store);
        // Use with: ui.add(table.show());
    }

    Ok(())
}
```

## Features

### Core Features

- **Async Data Loading**: Non-blocking data fetching with caching
- **Smart Caching**: Automatic caching with configurable page sizes
- **Cell-Level Access**: Efficient individual cell value retrieval
- **Mutation Support**: Update, insert, and delete operations
- **Column Metadata**: Rich column information with type and edit capabilities

### Framework-Specific Features

| Framework | Virtualization | Editing | Sorting | Filtering | Selection |
| --------- | -------------- | ------- | ------- | --------- | --------- |
| egui      | ✅ Built-in    | ✅ Yes  | ❌ No   | ❌ No     | ✅ Yes    |
| GPUI      | ✅ GPU-based   | ✅ Yes  | ❌ No   | ❌ No     | ✅ Yes    |
| Slint     | ✅ Built-in    | ✅ Yes  | ❌ No   | ❌ No     | ✅ Yes    |
| Tauri     | 🌐 Web-based   | ✅ Yes  | 🌐 JS   | 🌐 JS     | 🌐 JS     |
| Ratatui   | 📜 Terminal    | ❌ No   | ❌ No   | ❌ No     | ✅ Yes    |
| Cursive   | 📜 Terminal    | ❌ No   | ✅ Yes  | ❌ No     | ✅ Yes    |

## Examples

### egui Integration

```rust
use dataset_ui_adapters::egui_adapter::*;

let store = TableStore::new(your_dataset);
let mut table = EguiTable::new(store);

// In your egui update loop:
ui.add(table.show());
```

### Slint Integration

```rust
use dataset_ui_adapters::slint_adapter::*;

let store = TableStore::new(your_dataset);
let table = SlintTable::new(store);
let model = table.as_model_rc();

// In your .slint file:
// StandardTableView { rows: model }
```

### Ratatui Integration

```rust
use dataset_ui_adapters::ratatui_adapter::*;

let store = TableStore::new(your_dataset);
let mut adapter = RatatuiTableAdapter::new(store);
adapter.refresh_data().await;

// In your TUI render loop:
let table = adapter.create_table();
frame.render_stateful_widget(table, area, adapter.state_mut());
```

### Cursive Integration

```rust
use dataset_ui_adapters::cursive_adapter::*;

let store = TableStore::new(your_dataset);
let app = CursiveTableApp::new(store)?;
app.run()?;  // Runs complete TUI application
```

### Tauri Integration

```rust
use dataset_ui_adapters::tauri_adapter::*;

let store = TableStore::new(your_dataset);
let table = TauriTable::new(store);

tauri::Builder::default()
    .manage(table.manager().clone())
    .invoke_handler(tauri::generate_handler![
        get_table_columns,
        get_table_data,
        update_table_cell,
        get_table_row_count
    ])
    .run(tauri::generate_context!())?;
```

## Implementing DataSet

To use this crate with your own data, implement the `DataSet` trait:

```rust
use dataset_ui_adapters::*;

struct MyDataSet {
    // Your data source (database connection, API client, etc.)
}

#[async_trait::async_trait]
impl DataSet for MyDataSet {
    async fn row_count(&self) -> Result<usize> {
        // Return total number of rows
        todo!()
    }

    async fn column_info(&self) -> Result<Vec<ColumnInfo>> {
        // Return column metadata
        todo!()
    }

    async fn fetch_rows(&self, start: usize, count: usize) -> Result<Vec<TableRow>> {
        // Fetch a range of rows efficiently
        todo!()
    }

    async fn fetch_row(&self, index: usize) -> Result<TableRow> {
        // Fetch a single row
        todo!()
    }

    // Optional: implement mutation methods
    async fn update_cell(&self, row: usize, col: usize, value: CellValue) -> Result<()> {
        todo!()
    }
}
```

## Performance Considerations

### Caching Strategy

- **Page-based loading**: Configurable page sizes (default: 100 rows)
- **LRU-style cache**: Most recently accessed data stays in memory
- **Smart prefetching**: Anticipates scroll patterns for smooth UI

### Framework Optimizations

- **egui**: Leverages built-in virtualization for large datasets
- **GPUI**: GPU-accelerated rendering with efficient diff calculations
- **Slint**: Model-based reactivity with automatic change propagation
- **Tauri**: JSON serialization optimizations and pagination
- **Ratatui**: Lightweight terminal rendering with minimal memory usage
- **Cursive**: Built-in table widget with sorting and async data loading

### Memory Usage

- Configurable cache limits
- On-demand loading reduces memory footprint
- Efficient cell value storage with copy-on-write semantics

## Testing

Run tests for all frameworks:

```bash
cargo test --all-features
```

Run tests for specific framework:

```bash
cargo test --features egui
```

## Examples

Comprehensive example with all frameworks:

```bash
cargo run --example comprehensive_example --all-features
```

Framework-specific examples:

```bash
cargo run --example egui_example --features egui
cargo run --example slint_example --features slint
cargo run --example tui_example --features ratatui
cargo run --example cursive_example --features cursive
# etc.
```

## Contributing

1. Add new framework support by implementing the framework's table traits
2. All adapters should follow the same patterns established in existing code
3. Include comprehensive tests for new adapters
4. Update documentation and examples

## License

This project is licensed under the MIT License.
