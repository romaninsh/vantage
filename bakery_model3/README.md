# Bakery Model 3

Example bakery data model demonstrating Vantage 0.3 with CSV persistence — conditions,
relationships, and entity traversal.

## Entities

- **Bakery** — bakery locations with profit margins
- **Client** — customers with payment status, belongs to Bakery, has many Orders
- **Product** — bakery items with pricing
- **Order** — purchases linked to clients

## Running

```bash
cargo run -p bakery_model3 --example 0-intro
```

## CLI

Multi-source CLI for browsing and managing bakery data.

```
db [--debug] <source> <entity> [command]
```

**Sources:** `csv`, `surreal` **Entities:** `bakery`, `client`, `product`, `order`

### Commands

| Command           | CSV | SurrealDB | Description                 |
| ----------------- | --- | --------- | --------------------------- |
| `list`            | ✓   | ✓         | List all records as a table |
| `get`             | ✓   | ✓         | Show first record in detail |
| `count`           | ✓   | ✓         | Count records               |
| `add <id> <json>` |     | ✓         | Insert a record             |
| `delete <id>`     |     | ✓         | Delete a record by ID       |

### Examples

```bash
# List products from CSV files
cargo run -p bakery_model3 --example cli -- csv product list

# List clients from SurrealDB
cargo run -p bakery_model3 --example cli -- surreal client list

# Count bakeries
cargo run -p bakery_model3 --example cli -- surreal bakery count

# Get first order details
cargo run -p bakery_model3 --example cli -- surreal order get

# Insert a bakery
cargo run -p bakery_model3 --example cli -- surreal bakery add myid '{"name":"My Bakery","profit_margin":20}'

# Delete it
cargo run -p bakery_model3 --example cli -- surreal bakery delete myid

# Show SurrealDB queries (debug mode)
cargo run -p bakery_model3 --example cli -- --debug surreal product list
```

### SurrealDB connection

Set `SURREALDB_URL` or defaults to `cbor://root:root@localhost:8000/bakery/v2`.

## Usage

```rust
use bakery_model3::*;
use vantage_csv::{AnyCsvType, Csv};
use vantage_csv::operation::CsvOperation;
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
