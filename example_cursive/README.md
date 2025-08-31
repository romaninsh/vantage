# Dataset UI Adapters - Cursive Example

This example demonstrates how to use the dataset UI adapters with Cursive for terminal-based table display using the `cursive_table_view` widget.

## Features

- Interactive terminal table with Cursive TUI framework
- Sortable columns by clicking on headers
- Row selection with Enter key
- Dialog boxes for displaying selected row information
- Clean, cross-platform terminal interface

## Running the Example

```bash
cargo run
```

## Controls

- `↑/↓` or `Tab/Shift+Tab` - Navigate between rows
- `Enter` - Select a row (shows details in a dialog)
- `Left/Right` - Navigate between columns
- `q` or `Esc` - Quit the application
- `Refresh` button - Refresh data (placeholder functionality)

## Architecture

The example uses:

- `CursiveTableAdapter` - Adapter that bridges the generic `TableStore` with Cursive's TableView widget
- `TableStore` - Caching layer that manages data fetching and caching
- `MockProductDataSet` - Sample dataset implementation for demonstration
- `cursive_table_view` - Third-party widget providing table functionality for Cursive

## Implementation Details

The `CursiveTableAdapter` provides:

- Async data loading with caching using embedded Tokio runtime
- Sortable table columns with automatic type detection (numeric vs. string)
- Row selection callbacks and event handling
- Consistent interface with other UI adapters (GPUI, eGUI, Slint, Tauri, Ratatui)

The adapter follows the same pattern as other framework adapters in the project:

1. Wraps a `TableStore<D>` instance
2. Provides framework-specific rendering methods
3. Handles user interaction and state management
4. Maintains data consistency across UI updates

## Key Differences from Other Adapters

- Uses `usize` column identifiers instead of `String` (required by cursive_table_view)
- Embeds a Tokio runtime for handling async data operations
- Leverages cursive's built-in dialog system for user interactions
- Supports column sorting out of the box

This demonstrates how the same data layer can be adapted to work with different TUI frameworks while maintaining consistent behavior and data access patterns.
