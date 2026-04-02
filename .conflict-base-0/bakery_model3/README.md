# Bakery Model 3

Example bakery data model demonstrating Vantage 0.3 with CSV persistence — conditions, relationships, and entity traversal.

## Entities

- **Bakery** — bakery locations with profit margins
- **Client** — customers with payment status, belongs to Bakery, has many Orders
- **Product** — bakery items with pricing
- **Order** — purchases linked to clients

## Running

```bash
cargo run -p bakery_model3 --example 0-intro
```

## Usage

```rust
use bakery_model3::*;
use vantage_csv::{AnyCsvType, Csv};
use vantage_table::operation::Operation;
use vantage_dataset::prelude::ReadableDataSet;

let csv = Csv::new("bakery_model3/data");

// Filter with conditions
let mut paying = Client::csv_table(csv.clone());
paying.add_condition(paying["is_paying_client"].eq(AnyCsvType::new(true)));

// Traverse relationships
let orders = paying.get_ref_as::<Csv, Order>("orders")?;
for order in orders.list().await?.values() {
    println!("{}: {}", order.client_id, order.lines);
}
```
