# Iced Table Example

This example demonstrates how to use the `dataset-ui-adapters` crate with the Iced GUI framework to display tabular data.

## Features

- Displays a table with mock product data
- Click-to-edit cells functionality
- Basic table layout with fixed-width columns
- Integration with the dataset UI adapters framework

## Running the Example

Make sure you have Rust installed, then run:

```bash
cargo run
```

## What You'll See

The application will open a window showing:
- A header with the application title
- A table with 4 columns: Name, Calories, Price, Inventory
- 5 rows of sample product data (flux capacitors, time travel themed items)
- A "Load Data" button (currently loads hardcoded data)

## Interaction

- **Click any cell** to start editing it
- **Type** to change the cell value
- **Press Enter** or click elsewhere to stop editing
- **Click "Load Data"** to refresh the table (currently just ensures data is loaded)

## Code Structure

- `src/main.rs` - Main application using Iced's Application trait
- Uses `dataset-ui-adapters` with the `iced` feature enabled
- Demonstrates the message-passing architecture of Iced for table updates

## Current Limitations

This is a basic example with hardcoded data. In a real application, you would:
- Connect to actual data sources through the `DataSet` trait
- Implement proper async data loading
- Add more sophisticated error handling
- Implement actual persistence of edits

## Next Steps

To extend this example:
1. Replace hardcoded data with real DataSet implementation
2. Add sorting, filtering, and pagination
3. Implement proper async data loading with loading indicators
4. Add more table interaction features like row selection
