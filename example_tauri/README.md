# Tauri Example - Dataset UI Adapters

This example demonstrates how to integrate dataset-ui-adapters with Tauri to create cross-platform desktop applications with web-based UI and Rust backend.

## Features

- Read-only HTML table with Tauri 2.x backend
- Displays data from MockProductDataSet
- Clean, responsive web-based interface

## Running the Example

```bash
cd example_tauri/src-tauri
cargo run
```

Or with hot reload:

```bash
cd example_tauri
cargo tauri dev
```

## Architecture

- **Frontend**: HTML/CSS/JavaScript in WebView
- **Backend**: Rust with Tauri commands for data operations
- **Data**: MockProductDataSet with sample product data

## Commands

- `get_table_data()` - Retrieve table data
- `get_table_columns()` - Get column names

A minimal example demonstrating Tauri table integration with dataset-ui-adapters.
