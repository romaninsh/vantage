# Tauri Table Example

This example demonstrates how to use the `dataset-ui-adapters` crate with the Tauri framework to create a desktop application with web frontend that displays tabular data.

## Features

- **Hybrid Architecture**: Rust backend with HTML/CSS/JavaScript frontend
- **Tauri Commands**: Clean IPC between frontend and backend
- **Data Adapter Integration**: Uses the dataset-ui-adapters framework
- **Modern Tauri 1.6**: Latest stable Tauri with full desktop capabilities
- **Cross-platform**: Runs on Windows, macOS, and Linux
- **Web Technologies**: Familiar HTML/CSS/JS for UI development

## Running the Example

Make sure you have Rust installed, then run:

```bash
cd src-tauri
cargo run
```

## What You'll See

The application will open a desktop window showing:
- A professional web-based table interface
- 4 columns: Name, Calories, Price, Inventory
- 5 rows of sample product data (flux capacitors, time travel themed items)
- Refresh and Add Row buttons
- Click-to-edit cell functionality
- Row count display
- Responsive web styling

## Architecture

```
Frontend (HTML/JS) ↔ Tauri IPC ↔ Rust Backend ↔ DataSet Adapters
```

### Backend (Rust)
- `src-tauri/src/main.rs` - Main Tauri application with commands
- Uses `TauriTable` adapter to bridge data and IPC
- Thread-safe with `RwLock` for concurrent access
- Async command handlers for data operations

### Frontend (Web)
- `src/index.html` - Single-page web application
- Modern HTML5/CSS3/JavaScript
- Responsive table design
- Tauri API integration for backend communication

## Key Components

### Tauri Commands
```rust
#[tauri::command]
async fn get_table_data(
    table: tauri::State<'_, TauriTable<MockProductDataSet>>,
    page: Option<usize>,
    page_size: Option<usize>,
) -> Result<serde_json::Value, String>
```

### Frontend Integration
```javascript
const { invoke } = window.__TAURI__.tauri;
const result = await invoke('get_table_data', { page: 0, page_size: 100 });
```

## Available Commands

- **`get_table_data`** - Retrieve paginated table data
- **`get_table_columns`** - Get column definitions
- **`get_table_row_count`** - Get total number of rows
- **`update_table_cell`** - Update individual cell values

## File Structure

```
example_tauri/
├── src-tauri/
│   ├── src/main.rs          # Rust backend with Tauri commands
│   ├── Cargo.toml          # Rust dependencies
│   ├── tauri.conf.json     # Tauri configuration
│   └── build.rs            # Build script
└── src/
    └── index.html          # Web frontend
```

## Sample Data

The example displays product data including:
- **Flux Capacitor Cupcake** - 300 calories, $120, 50 in stock
- **DeLorean Doughnut** - 250 calories, $135, 30 in stock
- **Time Traveler Tart** - 200 calories, $220, 20 in stock
- **Enchantment Under the Sea Pie** - 350 calories, $299, 15 in stock
- **Hoverboard Cookies** - 150 calories, $199, 40 in stock

## Interaction

- **Click any cell** to edit its value (shows prompt dialog)
- **Refresh Data** button reloads the table
- **Add Row** button shows placeholder for adding functionality
- **Hover effects** provide visual feedback
- **Responsive design** adapts to window size

## Technical Details

### Thread Safety
- Uses `Arc<RwLock<>>` for thread-safe data access
- Multiple frontend clients can access data concurrently
- Proper state management for Tauri's multi-threaded environment

### Data Flow
```
MockProductDataSet → TableStore → TauriTable → Tauri Commands → Frontend
```

### IPC Communication
- JSON serialization for data transfer
- Error handling with `Result<T, String>` pattern
- Async commands for non-blocking operations

## Next Steps

To extend this example for production use:

1. **Real Data Sources**: Replace `MockProductDataSet` with database connections
2. **Advanced UI**: Add more sophisticated web components or frameworks
3. **Authentication**: Implement user authentication and authorization
4. **File Operations**: Add import/export capabilities
5. **Real-time Updates**: Implement WebSocket or similar for live data updates
6. **Error Handling**: Add comprehensive error handling and user feedback
7. **Testing**: Add unit tests for commands and integration tests

## Dependencies

### Backend
- `tauri 1.6` - Desktop application framework
- `serde` + `serde_json` - Serialization for IPC
- `dataset-ui-adapters` - Our table adapter framework
- `tokio` - Async runtime

### Frontend
- Native HTML5/CSS3/JavaScript
- Tauri API for backend communication
- No additional frameworks required

## Build Configuration

The application is configured through `tauri.conf.json`:
- Window size: 800x600
- Shell open permissions for external links
- Custom protocol support for development

This example showcases the power of Tauri's hybrid architecture, combining Rust's performance and safety with web technologies' flexibility and familiarity, all integrated seamlessly with the dataset-ui-adapters pattern.
