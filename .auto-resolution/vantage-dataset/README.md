# vantage-dataset

Internal crate providing traits for dataset operations across different data sources.

## Overview

This crate defines generic traits that can be implemented by datasets associated with specific data sources. Each trait represents a different capability or access pattern.

## Traits

### `ReadableDataSet`

For read-only data sources that can fetch and deserialize records.

```rust
// Example: CSV file dataset
let csv_data: CsvDataSet = CsvDataSet::new("users.csv");
let users: Vec<User> = csv_data.get().await?;
let first_user: User = csv_data.get_some().await?;
```

### `InsertableDataSet`

For append-only data sources like queues, logs, or event streams.

```rust
// Example: Message queue topic
let topic: Topic<Event> = queue.topic("user_events");
topic.insert(Event { user_id: 123, action: "login" }).await?;
```

### `WritableDataSet`

For full read-write data sources with update and delete capabilities. Extends `InsertableDataSet`.

```rust
// Example: Database table
let users: Table<User> = db.table("users");
users.update::<User, _>(|user| {
    user.last_seen = Utc::now();
}).await?;
users.delete().await?;
```

### `IndexableDataSet`

For key-value stores and indexed data sources that operate on specific identifiers.

```rust
// Example: Redis-like key-value store
let cache: KeyStore<String, User> = redis.keystore();
cache.get_by_key("user:123").await?;
cache.set_by_key("user:456", user).await?;
cache.delete_by_key("user:789").await?;
```

## Use Cases

- **Queue/Topic** (`Topic<Entity>`): Implements `InsertableDataSet` only
- **Read-only CSV**: Implements `ReadableDataSet` only
- **Database Table**: Implements `WritableDataSet` (which includes insertable)
- **Key-Value Store**: Implements `IndexableDataSet`
- **Document Store**: May implement multiple traits depending on capabilities

## Error Handling

All traits use a configurable `Error` associated type. The crate provides `DataSetError` for common error scenarios, built with `thiserror` for clean error handling.
