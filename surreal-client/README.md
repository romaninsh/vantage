# Surreal Client

A SurrealDB client library for Rust speaking native CBOR over WebSocket.

`ws://`, `wss://`, and `cbor://` URLs all route through the same CBOR engine — `ws://` is the canonical scheme, `cbor://` is accepted as an alias for backward compatibility.

## Features

- **CBOR WebSocket engine**: Native CBOR wire format. Preserves SurrealDB types (Datetime, Duration, RecordId, Bytes, Decimal) that JSON cannot carry faithfully.
- **JSON convenience methods**: `create`/`select`/`update`/`merge`/`patch`/`delete`/`query` accept and return `serde_json::Value`, transcoded to/from CBOR on the wire. Use `query_cbor` when fidelity matters.
- **Immutable Client Design**: Thread-safe, cloneable client with unique sessions.
- **Builder Pattern Connection**: Intuitive connection configuration.
- **Debug mode support**: Outputs requests/responses for debugging.
- **Multiple Authentication Methods**: Root, namespace, database, and JWT token auth.
- **Live Queries**: `live()` streams `CREATE` / `UPDATE` / `DELETE` notifications for a table or record.
- **Query Interface**: Execute raw SurrealQL with parameter binding.
- **Session Management**: Variables and state management per client.
- **Relation Support**: Create and query record relationships.
- **Transaction Support**: Execute multi-statement transactions.

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

DSN credentials sign in as root. Add `?auth=namespace` or `?auth=database` to sign in as a
namespace or database user instead.

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

// Scope authentication (the builder accepts it, but connect() currently
// returns "Unsupported authentication method")
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

## Live Queries

`live()` starts a `LIVE SELECT` on a table or record and returns a `LiveStream` of
`Notification`s. Each one carries the `Action` (`Create`, `Update` or `Delete`), the record id and
the record data as raw CBOR. It needs the WebSocket connection, which is the only network engine
this crate ships.

```rust
use surreal_client::Action;

let mut stream = client.live("users").await?;
while let Some(note) = stream.recv().await {
    match note.action {
        Action::Create | Action::Update => println!("changed: {:?}", note.record_id),
        Action::Delete => println!("deleted: {:?}", note.record_id),
    }
}
```

`LiveStream` also implements `futures::Stream`. Dropping it kills the server-side query in the
background; `stream.close().await?` kills it and waits for the reply.

## Pooling

Normally you have one client and one engine. You can clone your client,
but the engine remains the same. As a result, some queries may block
other queries.

There is no pool type yet. The `pool` feature only defines `PoolConfig`. To run queries in
parallel, call `connect()` once per connection; each call opens its own engine:

```rust
// DSN="ws://root:secret@localhost:8000/bakery/v1"
let conn = SurrealConnection::dsn(dsn)?;

let db1 = conn.clone().connect().await?;
let db2 = conn.connect().await?;

// Execute both queries simultaniously
tokio::try_join!(
    db1.query("sleep 1s", None),
    db2.create("user", Some(json!({"name": "John", "age": 25})))
)?;
```

## Vantage integration

This crate is the transport for `vantage-surrealdb`, which wraps the client as a `SurrealDB` data
source for its query builders:

```rust
let client = SurrealConnection::dsn(dsn)?.connect().await?;
let db = SurrealDB::new(client);

let select = SurrealSelect::new()
    .with_source("client")
    .with_condition(surreal_expr!("bakery = {}", (Thing::new("bakery", "hill_valley"))))
    .with_order(surreal_expr!("name"), Order::Asc);
// SELECT * FROM client WHERE bakery = bakery:hill_valley ORDER BY name

// Create delayed query - can be used inside another query
let deferred = db.defer(select.expr());

// Execute and get data right away
let data = select.get(&db).await?;
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

For Vantage tables, datasets and `Vista`, use `vantage-surrealdb`, which builds on this client.
