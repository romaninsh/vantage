# Dataset UI Adapters - GPUI Example

A complete example demonstrating how to use the Dataset UI Adapters library with GPUI framework for native desktop applications.

## Overview

This example shows how to integrate the `dataset-ui-adapters` library with GPUI to display tabular data in a native desktop application. It demonstrates the adapter pattern in action - the same data layer (`MockProductDataSet`) can work across different UI frameworks by simply changing the adapter implementation.

## Features

- **Native GPUI Table**: Uses the official GPUI Table component with proper theming
- **Real Data Integration**: Shows actual data from `MockProductDataSet` with product information
- **Proper Styling**: Follows GPUI theming system with proper light/dark mode support
- **Native Menu Integration**: Includes native macOS/Windows menu with Quit functionality
- **Async Data Loading**: Demonstrates async data fetching through the adapter layer

## Sample Data

The example displays a table with the following product data:

- Flux Capacitor Cupcake (300 cal, $1.20, 50 in stock)
- DeLorean Doughnut (250 cal, $1.35, 30 in stock)
- Time Traveler Tart (200 cal, $2.20, 20 in stock)
- Enchantment Under the Sea Pie (350 cal, $2.99, 15 in stock)
- Hoverboard Cookies (150 cal, $1.99, 40 in stock)

## Architecture

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│ MockProductDataSet │ → │   TableStore     │ → │ GpuiTableDelegate │
│   (Data Source)    │    │  (Caching Layer) │    │   (UI Adapter)    │
└─────────────────┘    └──────────────────┘    └─────────────────┘
                                                          │
                                                          ▼
                                                ┌─────────────────┐
                                                │   GPUI Table    │
                                                │  (UI Component) │
                                                └─────────────────┘
```

## Key Components

- **TableApp**: Main application struct containing the GPUI table entity
- **GpuiTableDelegate**: Adapter implementing GPUI's `TableDelegate` trait
- **MockProductDataSet**: Sample data source with bakery product information
- **TableStore**: Caching layer for efficient async data operations

## Dependencies

- `gpui` - The GPUI framework from Zed Industries
- `gpui-component` - Official UI components library for GPUI
- `dataset-ui-adapters` - The core adapter library (with GPUI feature enabled)
- `tokio` - Async runtime for data operations

## Building and Running

```bash
# From the example_gpui directory
cargo run
```

The application will open a native desktop window with:

- A properly themed table showing bakery product data
- Native menu bar with Quit option (Cmd+Q on macOS)
- Resizable window with minimum size constraints

## Menu Usage

- **Quit**: Use Cmd+Q (macOS) or the menu to quit the application
- Window closing behavior follows system defaults

## Code Structure

- `main.rs` - Complete GPUI application with proper initialization
- Uses `Root` wrapper for proper GPUI component integration
- Follows vantage-admin patterns for styling and layout
- Minimal dependencies and clean architecture

## Integration with Other UI Frameworks

This example demonstrates one of five UI framework integrations:

- `example_egui` - egui framework integration
- `example_gpui` - **This example** - GPUI framework integration
- `example_slint` - Slint framework integration
- `example_tauri` - Tauri framework integration

All examples use the same underlying data layer (`MockProductDataSet`) but with different UI adapters, showcasing the power and flexibility of the adapter pattern.

## Notes

- Table styling follows GPUI's theme system automatically
- Data is loaded asynchronously through the adapter
- The example demonstrates basic table functionality - sorting, scrolling, etc. are handled by the GPUI Table component
- Window management follows GPUI conventions
