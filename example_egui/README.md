# egui Table Example

This example demonstrates how to use the `dataset-ui-adapters` crate with the egui GUI framework to display tabular data using the powerful `egui-data-table` library.

## Features

- Professional data table with egui-data-table integration
- Click-to-edit cells functionality
- Undo/Redo support (built into egui-data-table)
- Row duplication and removal capabilities
- Keyboard navigation
- Show/Hide/Reorder columns
- Built-in clipboard support
- Modern egui 0.32 with latest data table features

## Running the Example

Make sure you have Rust installed, then run:

```bash
cargo run
```

## What You'll See

The application will open a window showing:
- A header with the application title
- A professional data table with advanced features
- 4 columns: Name, Calories, Price, Inventory
- 5 rows of sample product data (flux capacitors, time travel themed items)
- A "Refresh Data" button to reload the table
- Built-in table controls for sorting, editing, and row management

## Interaction

- **Click any cell** to start editing it
- **Double-click** for advanced editing features
- **Right-click** for context menu options
- **Use keyboard shortcuts** for navigation and editing
- **Drag column headers** to reorder columns
- **Use built-in controls** for adding/removing rows
- **Undo/Redo** with Ctrl+Z/Ctrl+Y

## Code Structure

- `src/main.rs` - Main application using eframe
- Uses `dataset-ui-adapters` with the `egui` feature enabled
- Implements the `RowViewer` trait for proper data table integration
- Demonstrates professional table features with minimal code

## Key Components

### EguiTableViewer
- Implements the `RowViewer` trait required by egui-data-table
- Handles cell display and editing logic
- Manages data conversion between CellValue and display formats

### EguiTable
- Wraps the DataTable and Viewer for easy usage
- Provides simple `show()` method for integration
- Handles data loading and refresh functionality

## Current Implementation

This example uses hardcoded placeholder data to demonstrate the table functionality. The data includes:
- Product names (Flux Capacitor Cupcake, DeLorean Doughnut, etc.)
- Nutritional information (calories)
- Pricing data
- Inventory counts

## Next Steps

To extend this example for production use:

1. **Connect Real Data**: Replace placeholder data with actual DataSet implementation
2. **Async Loading**: Add proper async data loading with loading indicators
3. **Persistence**: Implement actual data persistence for edits
4. **Validation**: Add input validation for cell editing
5. **Custom Styling**: Customize table appearance and behavior
6. **Advanced Features**: Utilize more egui-data-table features like custom cell renderers

## Dependencies

- `eframe 0.32` - Modern egui application framework
- `egui 0.32` - Immediate mode GUI library
- `egui-data-table 0.8` - Professional data table widget
- `dataset-ui-adapters` - Our table adapter framework

This example showcases the power of combining egui's immediate mode approach with a sophisticated data table component, providing a professional data editing experience.
