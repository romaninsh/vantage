# Slint Table Example

This example demonstrates how to use the `dataset-ui-adapters` crate with the Slint GUI framework to display tabular data using Slint's built-in `StandardTableView` widget.

## Features

- **StandardTableView Integration**: Uses Slint's native table widget
- **Data Adapter Pattern**: Demonstrates clean integration with the dataset-ui-adapters framework
- **Modern Slint 1.8**: Uses the latest Slint version with full feature support
- **Reactive UI**: Automatic data binding with Slint's reactive architecture
- **Cross-platform**: Runs on Windows, macOS, and Linux

## Running the Example

Make sure you have Rust installed, then run:

```bash
cargo run
```

## What You'll See

The application will open a window showing:
- A clean, professional table interface
- 4 columns: Name, Calories, Price, Inventory
- 5 rows of sample product data (flux capacitors, time travel themed items)
- Native scrolling and selection capabilities
- Built-in keyboard navigation support

## Code Structure

- `src/main.rs` - Main application entry point
- `ui/main.slint` - Slint UI definition with StandardTableView
- `build.rs` - Build script for Slint compilation
- Uses `dataset-ui-adapters` with the `slint` feature enabled

### Key Components

#### UI Definition (`main.slint`)
```slint
StandardTableView {
    columns: [
        { title: "Name" },
        { title: "Calories" },
        { title: "Price" },
        { title: "Inventory" }
    ];
    rows: root.table-rows;
}
```

#### Rust Integration (`main.rs`)
- Creates `MockProductDataSet` with sample data
- Uses `SlintTable` adapter to bridge between data and UI
- Converts adapter data to Slint's `StandardListViewItem` format
- Uses nested `ModelRc` structure for table rows

## Data Flow

```
MockProductDataSet → TableStore → SlintTable → ModelRc<SlintTableRow> → StandardTableView
```

## Sample Data

The example displays product data including:
- **Flux Capacitor Cupcake** - 300 calories, $120, 50 in stock
- **DeLorean Doughnut** - 250 calories, $135, 30 in stock
- **Time Traveler Tart** - 200 calories, $220, 20 in stock
- **Enchantment Under the Sea Pie** - 350 calories, $299, 15 in stock
- **Hoverboard Cookies** - 150 calories, $199, 40 in stock

## Architecture Benefits

### Clean Separation
- **Data Layer**: Your existing dataset implementations
- **Adapter Layer**: Handles format conversion and caching
- **UI Layer**: Pure Slint UI definitions

### Type Safety
- Rust's type system ensures data consistency
- Slint's compile-time UI validation
- No runtime type errors

### Performance
- Efficient data loading through the adapter pattern
- Native rendering performance with Slint
- Memory-efficient reactive updates

## Next Steps

To extend this example for production use:

1. **Real Data Sources**: Replace `MockProductDataSet` with actual data backends
2. **Interactive Features**: Add editing, sorting, and filtering capabilities
3. **Custom Styling**: Customize table appearance with Slint's styling system
4. **Error Handling**: Add proper error handling and loading states
5. **Async Operations**: Implement async data loading with progress indicators

## Dependencies

- `slint 1.8` - Modern declarative UI framework
- `dataset-ui-adapters` - Our table adapter framework with `slint` feature
- `slint-build 1.8` - Build-time Slint compiler

## Technical Notes

### StandardTableView Requirements
- Expects `[[StandardListViewItem]]` for row data
- Each row is a `ModelRc<StandardListViewItem>`
- Columns defined with simple `{ title: "Name" }` structure

### Build Process
- Slint files are compiled at build time via `build.rs`
- Generated Rust code provides type-safe UI bindings
- No runtime parsing or validation overhead

This example showcases the elegance of combining Slint's declarative UI approach with Rust's type safety and the dataset-ui-adapters pattern for clean, maintainable table implementations.
