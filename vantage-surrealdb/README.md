# Vantage SurrealDB

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
use vantage_surrealdb::surreal_client::Connection;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect using builder pattern
    let client = Connection::new()
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
let client = Connection::dsn("ws://root:root@localhost:8000/my_ns/my_db")?
    .connect()
    .await?;
```

### Multiple Authentication Methods

```rust
// Root authentication
let client = Connection::new()
    .url("ws://localhost:8000")
    .auth_root("admin", "password")
    .connect().await?;

// Namespace authentication
let client = Connection::new()
    .url("ws://localhost:8000")
    .namespace("my_ns")
    .auth_namespace("ns_user", "ns_pass")
    .connect().await?;

// Database authentication
let client = Connection::new()
    .url("ws://localhost:8000")
    .namespace("my_ns")
    .database("my_db")
    .auth_database("db_user", "db_pass")
    .connect().await?;

// Scope authentication
let client = Connection::new()
    .url("ws://localhost:8000")
    .auth_scope("my_ns", "my_db", "user_scope", json!({
        "email": "user@example.com",
        "password": "user_password"
    }))
    .connect().await?;

// JWT token authentication
let client = Connection::new()
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

## Development

### Running Tests

#### Unit Tests
```bash
cargo test -p vantage-surrealdb
```

#### Integration Tests

Integration tests require a running SurrealDB instance:

1. **Start SurrealDB**:
   ```bash
   # Install SurrealDB if not already installed
   curl --proto '=https' --tlsv1.2 -sSf https://install.surrealdb.com | sh

   # Start in-memory instance for testing
   surreal start --log trace --user root --pass root memory
   ```

2. **Run integration tests**:
   ```bash
   # Run all integration tests
   cargo test --test surrealdb

   # Run specific test
   cargo test --test surrealdb test_basic_connection

   # Run with output
   cargo test --test surrealdb -- --nocapture
   ```

3. **Test Categories**:
   - `test_basic_connection` - Basic connectivity and version check
   - `test_http_connection` - HTTP protocol connection
   - `test_dsn_connection` - DSN string parsing and connection
   - `test_crud_operations` - Create, read, update, delete operations
   - `test_bulk_operations` - Bulk insert and complex queries
   - `test_session_variables` - Session variable management
   - `test_relations` - Record relationship operations
   - `test_transactions` - Multi-statement transactions
   - `test_complex_queries` - Advanced SurrealQL queries
   - `test_client_cloning` - Client cloning and session isolation
   - `test_error_handling` - Error cases and edge conditions
   - `test_import_export` - Database import/export (HTTP only, ignored by default)

#### Docker Testing

For consistent testing environment:

```bash
# Start SurrealDB in Docker
docker run --rm -p 8000:8000 surrealdb/surrealdb:latest \
  start --log trace --user root --pass root memory

# Run tests
cargo test --test surrealdb
```

### Test Configuration

Tests use these default settings:
- **URL**: `ws://localhost:8000`
- **Credentials**: `root/root`
- **Namespace**: `test`
- **Database**: `integration`

To use different settings, modify the constants in `tests/surrealdb.rs`.

### CI/CD

For continuous integration, ensure SurrealDB is available:

```yaml
# GitHub Actions example
services:
  surrealdb:
    image: surrealdb/surrealdb:latest
    ports:
      - 8000:8000
    options: --health-cmd "curl -f http://localhost:8000/health || exit 1"
    cmd: start --log trace --user root --pass root memory

steps:
  - name: Run integration tests
    run: cargo test --test surrealdb
```

## Architecture

### Connection Flow
```
Connection (Builder) → Authentication → Engine Creation → Immutable Client
```

1. **Connection Builder**: Configure URL, auth, namespace/database
2. **Authentication**: Establish credentials with SurrealDB
3. **Engine Creation**: Create HTTP or WebSocket engine based on URL scheme
4. **Client Creation**: Return immutable, thread-safe client

### Thread Safety
- Client is immutable after creation
- Session state is protected by `Arc<Mutex<SessionState>>`
- Message IDs are atomic and thread-safe
- Multiple clients can share the same underlying connection

### Session Management
- Each client clone gets a fresh session
- Session variables are isolated per client
- Namespace/database context is maintained per session
- Automatic cleanup on client drop

## Error Handling

All operations return `Result<T, SurrealError>` with comprehensive error types:

```rust
use vantage_surrealdb::surreal_client::SurrealError;

match client.query("INVALID SQL", None).await {
    Ok(result) => println!("Success: {:?}", result),
    Err(SurrealError::Protocol(msg)) => println!("Protocol error: {}", msg),
    Err(SurrealError::Connection(msg)) => println!("Connection error: {}", msg),
    Err(SurrealError::Auth(msg)) => println!("Authentication error: {}", msg),
    Err(err) => println!("Other error: {}", err),
}
```

## License

This project is licensed under the MIT OR Apache-2.0 license.
