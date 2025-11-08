# Bakery Model 3

A complete example implementation demonstrating Vantage 0.3 architecture with SurrealDB. This crate showcases entity definitions, table patterns, and real database integration using the new vantage-table ecosystem.

## Overview

Bakery Model 3 represents the migration from Vantage 0.2's monolithic approach to 0.3's modular, protocol-based design. It demonstrates how to define entities, create tables, and work with real SurrealDB data using the modern Vantage stack.

## Entities

The bakery model includes four main entities representing a typical bakery business:

- **Bakery** - Individual bakery locations with profit margins
- **Client** - Customers with contact information and payment status
- **Product** - Bakery items with pricing and nutritional information
- **Order** - Customer purchases linking clients and products

## Quick Start

```rust
use bakery_model3::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to SurrealDB
    bakery_model3::connect_surrealdb().await?;

    // Get client table
    let client_table = Client::table();

    // Execute query and get data
    let client_query = client_table.select_surreal();
    let ds = surrealdb();
    let clients = ds.get(client_query).await;

    // Process results
    if let serde_json::Value::Array(client_array) = clients {
        for client in client_array {
            if let (Some(name), Some(email)) = (client.get("name"), client.get("email")) {
                println!("Client: {} ({})", name.as_str().unwrap(), email.as_str().unwrap());
            }
        }
    }

    Ok(())
}
```

## Entity Definitions

### Client

```rust
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Client {
    pub id: Option<String>,
    pub name: String,
    pub email: String,
    pub is_paying_client: bool,
    pub metadata: Option<serde_json::Value>,
}

impl Client {
    pub fn table() -> Table<SurrealDB, Client> {
        Table::new("client", surrealdb())
            .with_column("id", "id")
            .with_column("name", "name")
            .with_column("email", "email")
            .with_column("is_paying_client", "is_paying_client")
            .with_column("metadata", "metadata")
    }
}
```

### Product

```rust
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Product {
    pub id: Option<String>,
    pub name: String,
    pub calories: i32,
    pub price: i32,
    pub bakery_id: Option<String>,
}

impl Product {
    pub fn table() -> Table<SurrealDB, Product> {
        Table::new("product", surrealdb())
            .with_column("id", "id")
            .with_column("name", "name")
            .with_column("calories", "calories")
            .with_column("price", "price")
            .with_column("bakery_id", "bakery_id")
    }
}
```

## Database Setup

### Requirements

- SurrealDB server running on `ws://localhost:8000`
- Or set `SURREALDB_URL` environment variable

### Setup Scripts

The `vantage-surrealdb` crate provides setup scripts:

```bash
cd ../vantage-surrealdb

# Start SurrealDB server
./run.sh

# In another terminal, populate with sample data
./ingress.sh
```

This creates the `bakery/v2` database with sample clients, products, and orders.

## Migration Status (0.2 → 0.3)

✅ **Completed**:

- Entity definitions adapted for SurrealDB (string IDs, embedded documents)
- Table trait patterns established using vantage-table
- Expression system integrated via vantage-expressions
- Field accessor methods returning expressions instead of column objects
- Database connection initialization (DSN pattern implemented, working)
- Query building and execution (SurrealTableExt methods working)
- Basic data retrieval (select_surreal(), select_surreal_column() working)

⏳ **In Progress**:

- CRUD operations (.get(), .insert(), .count() methods - waiting for vantage-table impl)
- Condition system (.with_condition(), field comparisons - not yet implemented)
- Relationship traversal (.ref_orders(), .with_many() - not yet implemented)
- Dynamic entity loading (.get_some_as::<T>() - not yet implemented)

## Architecture Changes

Key differences from Vantage 0.2:

- **Table definitions**: Now use builder pattern instead of static initialization
- **Field accessors**: Return Expression instead of typed column objects
- **Database features**: SurrealDB embedded documents properly supported
- **Connection management**: Moved to dedicated client libraries (surreal-client)
- **DSN pattern**: Connection strings like `ws://user:pass@host:port/namespace/database`
- **Query execution**: Uses `ds.get(query)` pattern instead of `table.get().await`

## Examples

Run the intro example to see basic functionality:

```bash
cargo run --example 0-intro
```

Expected output:

```
email: biff-3293@hotmail.com, client: Biff Tannen
email: doc@brown.com, client: Doc Brown
email: marty@gmail.com, client: Marty McFly
```

## Integration with UI Adapters

This model serves as the data source for all UI framework examples:

```rust
use bakery_model3::*;
use vantage_ui_adapters::*;

// Connect and create adapter
bakery_model3::connect_surrealdb().await?;
let client_table = Client::table();
let client_table_adapter = VantageTableAdapter::new(client_table).await;
let store = TableStore::new(client_table_adapter);

// Use with any UI framework
let egui_table = egui_adapter::EguiTable::new(store.clone()).await;
let slint_table = slint_adapter::SlintTable::new(store.clone()).await;
// etc.
```

## Future Features

Planned implementations for full 0.3 compatibility:

- **Conditions**: `client_table.with_condition(client_table.is_paying_client().eq(true))`
- **Relationships**: `client_table.ref_orders()` to traverse to Order table
- **Aggregation**: `order_table.count()`, `product_table.sum("price")`
- **Custom Methods**: `order_table.generate_report()` for business logic
- **Dynamic Loading**: `client_table.get_some_as::<MiniClient>()` for partial data

## Contributing

This model serves as the reference implementation for Vantage 0.3 patterns. When adding features:

1. Follow the async-first, protocol-based design
2. Maintain compatibility with vantage-table patterns
3. Add examples demonstrating new functionality
4. Update migration status in this README

## License

MIT License - see LICENSE file for details.
