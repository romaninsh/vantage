# Dataset UI Adapters - Ratatui Example

This example demonstrates how to use the dataset UI adapters with Ratatui for terminal-based table display.

## Features

- Display tabular data in a terminal interface
- Navigate through rows using arrow keys or vim-like navigation (j/k)
- Scrollbar for visual feedback on position
- Clean, minimal terminal UI

## Running the Example

```bash
cargo run
```

## Controls

- `↑` or `k` - Move up one row
- `↓` or `j` - Move down one row
- `r` - Refresh data
- `q` or `Esc` - Quit the application

## Architecture

The example uses:

- `RatatuiTableAdapter` - Adapter that bridges the generic `TableStore` with Ratatui's Table widget
- `TableStore` - Caching layer that manages data fetching and caching
- `MockProductDataSet` - Sample dataset implementation for demonstration

This demonstrates how the same data layer can be used across different UI frameworks while maintaining consistent behavior.

## Implementation Details

The `RatatuiTableAdapter` provides:

- Async data loading with caching
- Terminal-friendly table rendering
- Navigation state management
- Consistent interface with other UI adapters (GPUI, eGUI, Slint, Tauri)

The adapter follows the same pattern as other framework adapters in the project:

1. Wraps a `TableStore<D>` instance
2. Provides framework-specific rendering methods
3. Handles user interaction and state management
4. Maintains data consistency across UI updates
