# Surreal Client

A comprehensive SurrealDB client library for Rust with support for both HTTP and WebSocket connections.

## Features

- **Dual Protocol Support**: Connect via HTTP or WebSocket
- **Immutable Client Design**: Thread-safe, cloneable client with unique sessions
- **Builder Pattern Connection**: Intuitive connection configuration
- **Multiple Authentication Methods**: Root, namespace, database, scope, and JWT token auth
- **Full CRUD Operations**: Create, read, update, delete with type safety
- **Query Interface**: Execute raw SurrealQL with parameter binding
- **Session Management**: Variables and state management per client
- **Relation Support**: Create and query record relationships
- **Transaction Support**: Execute multi-statement transactions
- **Import/Export**: Database backup and restore (HTTP only)

## Quick Start

### Basic Connection

```rust
use surreal_client::SurrealConnection;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect using builder pattern
    let client = SurrealConnection::new()
        .url("ws://localhost:8000")
        .namespace("my_namespace")
        .database("my_database")
        .auth_root("root", "root")
        .connect()
        .await?;

    // Client is now immutable and ready to use
    let version = client.version().await?;
    println!("Connected to SurrealDB {}", version);

    // Perform CRUD operations
    let user = client.create("users:john", Some(json!({
        "name": "John Doe",
        "email": "john@example.com",
        "age": 30
    }))).await?;

    println!("Created user: {:?}", user);
    Ok(())
}
```

### DSN Connection

```rust
let client = SurrealConnection::dsn("ws://root:root@localhost:8000/my_ns/my_db")?
    .connect()
    .await?;
```

### Multiple Authentication Methods

```rust
// Root authentication
let client = SurrealConnection::new()
    .url("ws://localhost:8000")
    .auth_root("admin", "password")
    .connect().await?;

// Namespace authentication
let client = SurrealConnection::new()
    .url("ws://localhost:8000")
    .namespace("my_ns")
    .auth_namespace("ns_user", "ns_pass")
    .connect().await?;

// Database authentication
let client = SurrealConnection::new()
    .url("ws://localhost:8000")
    .namespace("my_ns")
    .database("my_db")
    .auth_database("db_user", "db_pass")
    .connect().await?;

// Scope authentication
let client = SurrealConnection::new()
    .url("ws://localhost:8000")
    .auth_scope("my_ns", "my_db", "user_scope", json!({
        "email": "user@example.com",
        "password": "user_password"
    }))
    .connect().await?;

// JWT token authentication
let client = SurrealConnection::new()
    .url("ws://localhost:8000")
    .auth_token("your_jwt_token_here")
    .connect().await?;
```

## CRUD Operations

```rust
// Create
let user = client.create("users:alice", Some(json!({
    "name": "Alice",
    "email": "alice@example.com"
}))).await?;

// Read
let users = client.select("users").await?;
let alice = client.select("users:alice").await?;

// Update
let updated = client.update("users:alice", Some(json!({
    "age": 25
}))).await?;

// Delete
let deleted = client.delete("users:alice").await?;

// Insert (bulk)
let products = client.insert("products", json!([
    {"name": "Laptop", "price": 999.99},
    {"name": "Mouse", "price": 29.99}
])).await?;

// Merge
let merged = client.merge("users:alice", json!({
    "last_login": "2024-01-15T10:00:00Z"
})).await?;

// Upsert
let upserted = client.upsert("users:bob", Some(json!({
    "name": "Bob",
    "email": "bob@example.com"
}))).await?;
```

## Query Interface

```rust
// Simple query
let results = client.query("SELECT * FROM users WHERE age > 18", None).await?;

// Parameterized query
let results = client.query(
    "SELECT * FROM users WHERE age > $min_age AND city = $city",
    Some(json!({
        "min_age": 21,
        "city": "New York"
    }))
).await?;

// Session variables
client.let_var("user_id", json!("user123")).await?;
let results = client.query(
    "SELECT * FROM posts WHERE author = $user_id",
    None
).await?;
client.unset("user_id").await?;
```

## Relations

```rust
// Create relation
let like = client.relate(
    "users:alice",
    "likes",
    "posts:post1",
    Some(json!({"timestamp": "2024-01-15T10:00:00Z"}))
).await?;

// Query relations
let user_likes = client.query("SELECT * FROM users:alice->likes->posts", None).await?;
let post_likes = client.query("SELECT * FROM posts:post1<-likes<-users", None).await?;
```

## Client Cloning

Each cloned client has its own session state:

```rust
let client1 = connection.connect().await?;
let client2 = client1.clone(); // Independent session

// Each client can have different session variables
client1.let_var("role", json!("admin")).await?;
client2.let_var("role", json!("user")).await?;

// Both clients share the same connection but have separate sessions
```

## Pooling

Normally you have one client and one engine. You can clone your client,
but the engine remains the same. As a result, some queries may block
other queries.

To avoid this, you can use `SurrealPool`:

```rust
// DSN="ws://root:secret@localhost:8000/bakery/v1?param=X"
let pool = SurrealConnection::dsn(dsn).pool(10);

let db1 = pool.connect().await?;
let db2 = pool.connect().await?;

// Execute both queries simultaniously
tokio::try_join!(
    db1.query("sleep 1s", None),
    db2.create("user", json!({"name": "John", "age": 25}))
)?
```

## Vantage integration

This crate is designed to work with the Vantage query builders:

```rust
let db = SurrealConnection::dsn(dsn).connect().await?;
let select = SurrealSelect::new()
    .with_source("client")
    .with_condition(expr!("bakery = {}", Thing::new("bakery", "hill_valley")))
    .with_order_by("name", true);
// Second query: SELECT * FROM client WHERE bakery = bakery:hill_valley order by name

// Create delayed query - can be used inside another query
let associated_query = db.defer(&select).await?;

// Execute and get data right away
let data = db.get(&select).await?;

// Also - can be executed directly
let same_data = associated_query.get().await?;
```

## Error Handling

All operations return `Result<T, SurrealError>` with comprehensive error types:

```rust
use surreal_client::SurrealError;

match client.query("INVALID SQL", None).await {
    Ok(result) => println!("Success: {:?}", result),
    Err(SurrealError::Protocol(msg)) => println!("Protocol error: {}", msg),
    Err(SurrealError::Connection(msg)) => println!("Connection error: {}", msg),
    Err(SurrealError::Auth(msg)) => println!("Authentication error: {}", msg),
    Err(err) => println!("Other error: {}", err),
}
```

TODO: Integration with Vantage models and DataSets.
