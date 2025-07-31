This is a blanked implementation of simple RPC client with
little number of dependencies.

## Connecting

```rust
// DSN="ws://root:secret@localhost:8000/bakery/v1?param=X"
let db = SurrealConnection::dsn(dsn).connect().await?;

// Create record
db.create("user", json!({"name": "John", "age": 25})).await?;
```

`db` above has is a `SurrealClient` struct, which is a lightweight struct,
that you can clone all over your application. The actual heavy-lifting is
done by the `engine`, which implements connectivity with the database
over WebSocket/RPC or HTTP.

To keep things simple - you are not allowed to change authentication
or switch between namespaces or databases from `SurrealClient`.

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
    .with_source("client", None)
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

TODO: Integration with Vantage models and DataSets.
