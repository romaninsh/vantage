# Vantage SurrealDB

SurrealDB integration for the Vantage framework with expression execution and comprehensive error
handling.

## Quick Start

```rust
use surreal_client::SurrealConnection;
use vantage_surrealdb::{surreal_expr, surrealdb::SurrealDB};
use vantage_expressions::ExprDataSource;

// Connect
let client = SurrealConnection::new()
    .url("ws://localhost:8000/rpc")
    .namespace("test").database("test")
    .auth_root("root", "root")
    .connect().await?;

let db = SurrealDB::new(client);

// Execute expressions
let result = db.execute(&surreal_expr!("RETURN {}", 42)).await?;
println!("Result: {:?}", result.value());
```

## Expression Execution

```rust
// Simple queries
let users = db.execute(&surreal_expr!("SELECT * FROM users")).await?;

// Parameterized queries
let user = db.execute(&surreal_expr!(
    "CREATE user:test SET name = {}, age = {}",
    "Alice", 25
)).await?;

// Different result types
let names = db.execute(&surreal_expr!("SELECT VALUE name FROM users")).await?;   // Array of values
let single = db.execute(&surreal_expr!("SELECT * FROM ONLY user:1")).await?;    // Single object
let count = db.execute(&surreal_expr!("RETURN count(SELECT * FROM users)")).await?; // Direct value
```

## Error Handling

```rust
// Query errors (ERR status from SurrealDB)
let duplicate = surreal_expr!("CREATE user:1 SET name = 'duplicate'");
match db.execute(&duplicate).await {
    Err(e) if e.to_string().contains("SurrealDB query failed") => {
        println!("Database error: {}", e);
    }
    _ => {}
}

// Parse errors (invalid syntax)
let invalid = surreal_expr!("SELECT =======");
match db.execute(&invalid).await {
    Err(e) if e.to_string().contains("Parse error") => {
        println!("Syntax error: {}", e);
    }
    _ => {}
}
```

## Features

- **Type-safe expressions** with `surreal_expr!` macro
- **Automatic result extraction** from SurrealDB response format
- **Rich error handling** for both query and protocol failures
- **CBOR protocol** for high performance
- **Integration** with vantage-expressions for cross-database queries

## Testing

```bash
# Start SurrealDB
surreal start --bind 0.0.0.0:8000 --user root --pass root memory

# Run tests
cargo test -p vantage-surrealdb
```

See `examples/expr.rs` for comprehensive usage patterns.
