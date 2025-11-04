# Vantage SurrealDB

Vantage integration for SurrealDB, providing query builder functionality and ORM-like features for SurrealDB databases.

This crate bridges the `surreal-client` (low-level SurrealDB client) with the Vantage query building ecosystem.

## Features

- **Query Builder Integration**: Use Vantage query builders with SurrealDB
- **Cross-Database Queries**: Combine SurrealDB queries with other databases
- **Expression Support**: Full support for Vantage expressions and parameters
- **Type Safety**: Rust type system integration with SurrealDB operations
- **Deferred Queries**: Support for lazy query execution

## Quick Start

```rust
use surreal_client::SurrealConnection;
use vantage_surrealdb::prelude::*;
use vantage_expressions::expr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect using surreal-client
    let db = SurrealConnection::dsn("ws://root:root@localhost:8000/bakery/v1")?
        .connect()
        .await?;

    // Create Vantage data source
    let ds = SurrealDB::new(db);

    // Use Vantage query builders
    let select = SurrealSelect::new()
        .with_source("client")
        .with_field("name")
        .with_condition(expr!("bakery = {}", Thing::new("bakery", "hill_valley")))
        .with_order_by("name", true);

    // Execute with query builder
    let data = ds.get(select).await?;

    // Or create deferred query for use in other expressions
    let deferred = ds.defer(select).await?;

    Ok(())
}
```

## Development

Tests require a running SurrealDB instance. See `surreal-client` documentation for setup instructions.

```bash
cargo test -p vantage-surrealdb
```

## License

This project is licensed under the MIT OR Apache-2.0 license.
