# Vantage Dataset

A composable data abstraction framework for Rust that enables building type-safe CRUD operations
across different data sources. Provides foundational traits for implementing dataset operations that
work seamlessly with CSV files, message queues, databases, key-value stores, and in-memory
collections.

## Example Use

```rust,ignore
use vantage_dataset::{ImDataSource, ImTable, InsertableDataSet, ReadableDataSet};
use serde::{Deserialize, Serialize};

// Define your entity
#[derive(Serialize, Deserialize, Clone)]
struct User {
    name: String,
    email: String,
    age: u32,
}

// Work with any data source through common interface
let im_source = ImDataSource::new();
let users: ImTable<User> = ImTable::new(&im_source, "users");

// Insert and retrieve with type safety
let id = users.insert_return_id(&User {
    name: "Alice".to_string(),
    email: "alice@example.com".to_string(),
    age: 30,
}).await?;

let all_users = users.list().await?;
let alice: Option<User> = users.get(id).await?;
```

The trait system allows the same code patterns to work across vastly different storage backends -
from local CSV files to distributed databases.

## Features

- **Two-layer trait system**: [`ValueSet`] for raw values, [`DataSet`] for typed entities
- **Capability-based traits**: [`ReadableDataSet`], [`InsertableDataSet`], [`WritableDataSet`]
  represent what operations a data source supports
- **Type-safe operations**: Work with native Rust structs while maintaining storage abstraction
- **Active records**: `ActiveEntitySet` and [`ActiveRecordSet`] wrap a loaded row so you can edit
  it in place and `save()` it back
- **Value-level access**: [`ValueSet`] traits for working with raw JSON-like values
- **In-memory implementation**: [`ImTable`] for local caching and testing scenarios
- **Cross-dataset imports**: an `Importable` trait for moving data between storage types (no
  implementations yet)
- **Enterprise-ready**: Built for large codebases requiring storage migration and refactoring

Vantage Dataset solves the problem of data source abstraction by providing a unified interface that
adapts to each storage backend's actual capabilities. Unlike traditional ORMs that force a
lowest-common-denominator approach, Dataset traits let you express exactly what operations your data
source supports.

## Trait Architecture

The framework is built on a two-layer trait hierarchy:

### ValueSet Foundation

The [`ValueSet`] layer works with raw storage values and defines the foundational types:

```rust,ignore
// Foundation - defines ID and Value types
trait ValueSet {
    type Id: Send + Sync + Clone;        // String, UUID, Thing, etc.
    type Value: Send + Sync + Clone;     // JSON Value, CBOR, etc.
}

// Value-level operations - work with raw storage data
trait ReadableValueSet: ValueSet {
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Record<Self::Value>>>;
    async fn get_value(&self, id: impl Into<Self::Id>) -> Result<Option<Record<Self::Value>>>;
    async fn get_some_value(&self) -> Result<Option<(Self::Id, Record<Self::Value>)>>;
}

trait WritableValueSet: ValueSet {
    async fn insert_value(&self, id: impl Into<Self::Id>, record: &Record<Self::Value>) -> Result<Record<Self::Value>>;
    async fn replace_value(&self, id: impl Into<Self::Id>, record: &Record<Self::Value>) -> Result<Record<Self::Value>>;
    async fn patch_value(&self, id: impl Into<Self::Id>, partial: &Record<Self::Value>) -> Result<Record<Self::Value>>;
    async fn delete(&self, id: impl Into<Self::Id>) -> Result<()>;
    async fn delete_all(&self) -> Result<()>;
}

trait InsertableValueSet: ValueSet {
    async fn insert_return_id_value(&self, record: &Record<Self::Value>) -> Result<Self::Id>;
}

trait ActiveRecordSet: ReadableValueSet + WritableValueSet {
    async fn get_value_record(&self, id: impl Into<Self::Id>) -> Result<Option<ActiveRecord<'_, Self>>>;
    async fn list_value_records(&self) -> Result<Vec<ActiveRecord<'_, Self>>>;
}
```

### DataSet Entity Layer

The [`DataSet`] layer adds entity-awareness on top of ValueSet:

```rust,ignore
// Entity-aware operations built on ValueSet
trait DataSet<E: Entity>: ValueSet {}

// Capability traits - implement what your data source supports
trait ReadableDataSet<E>: DataSet<E> {
    async fn list(&self) -> Result<IndexMap<Self::Id, E>>;
    async fn get(&self, id: impl Into<Self::Id>) -> Result<Option<E>>;

    async fn get_some(&self) -> Result<Option<(Self::Id, E)>>;
}

trait InsertableDataSet<E>: DataSet<E> {
    async fn insert_return_id(&self, entity: &E) -> Result<Self::Id>;
}

trait WritableDataSet<E>: DataSet<E> {
    async fn insert(&self, id: impl Into<Self::Id>, entity: &E) -> Result<E>;
    async fn replace(&self, id: impl Into<Self::Id>, entity: &E) -> Result<E>;
    async fn patch(&self, id: impl Into<Self::Id>, partial: &E) -> Result<E>;
    async fn patch_changed(&self, id: impl Into<Self::Id>, before: &E, after: &E) -> Result<()>;
}

trait ActiveEntitySet<E>: ReadableDataSet<E> + WritableDataSet<E> {
    async fn get_entity(&self, id: impl Into<Self::Id>) -> Result<Option<ActiveEntity<'_, Self, E>>>;
    async fn list_entities(&self) -> Result<Vec<ActiveEntity<'_, Self, E>>>;
    async fn get_some_entity(&self) -> Result<Option<ActiveEntity<'_, Self, E>>>;
    fn new_entity(&self, id: Self::Id, entity: E) -> ActiveEntity<'_, Self, E>;
}
```

## Data Source Capabilities

Different storage backends implement different combinations of traits based on their natural
capabilities:

- **CSV Files**: [`ReadableValueSet`] + [`ReadableDataSet`] - immutable data source
- **Message Queues**: [`InsertableDataSet`] only - append-only event streams
- **Database Tables**: Full [`ValueSet`] + [`DataSet`] hierarchy - complete CRUD operations
- **In-Memory Collections**: All traits - complete local storage with change tracking
- **Key-Value Stores**: [`ValueSet`] traits with atomic operations

This design prevents runtime errors by making data source limitations visible at compile time.

## Value-Level Operations

Work directly with storage values without entity deserialization for performance-critical scenarios:

```rust,ignore
// Work directly with raw storage values
let raw_values = users.list_values().await?;
let user_value = users.get_value(user_id.clone()).await?;

// Patch specific fields without full entity round-trip
let partial_update: Record<Value> = json!({ "last_login": "2024-01-15T10:30:00Z" }).into();
users.patch_value(user_id.clone(), &partial_update).await?;

// Delete operations at value level
users.delete(user_id).await?;
users.delete_all().await?;
```

Value-level operations are essential for:

- Cross-database migrations where type conversions may be complex
- Performance-critical scenarios avoiding entity serialization overhead
- Generic operations that work with any storage format

## Record Pattern

The `ActiveEntitySet` and [`ActiveRecordSet`] traits wrap a loaded row in [`ActiveEntity`] or
`ActiveRecord` for interactive editing. Both are implemented automatically for any set that is
readable and writable:

```rust,ignore
// Get entity wrapped in ActiveEntity
let mut user_record = users.get_entity(user_id.clone()).await?.unwrap();

// Modify through mutable deref
user_record.email = "newemail@example.com".to_string();
user_record.age = 31;

// Write it back
user_record.save().await?;

// Or work with raw values
let mut value_record = users.get_value_record(user_id).await?.unwrap();
value_record.insert("status".to_string(), json!("active"));
value_record.save().await?;
```

`save()` is a full replace: it creates the row if missing and drops fields absent from the
in-memory copy. To write only the fields that changed, use
`WritableDataSet::patch_changed(id, &before, &after)`, which compares two entity values and patches
the difference (and writes nothing when they are equal).

## In-Memory Implementation

The [`ImTable`] provides complete local storage implementation:

```rust,ignore
use vantage_dataset::{ImDataSource, ImTable};

let data_source = ImDataSource::new();
let users: ImTable<User> = ImTable::new(&data_source, "users");

// Full CRUD operations in memory
let id = users.insert_return_id(&user).await?;
let all_users = users.list().await?;
let mut user_record = users.get_entity(id).await?.unwrap();
user_record.name = "Updated Name".to_string();
user_record.save().await?;
```

In-memory implementation is perfect for:

- Testing scenarios requiring real async trait behavior
- Local caching layers in distributed architectures
- Rapid prototyping without external dependencies
- Development environments

## Cross-Dataset Operations

Import data between different storage types while preserving type safety:

```rust,ignore
// Import from CSV to database
let csv_users: CsvFile<User> = CsvFile::new(csv_source, "users.csv");
let mut db_users: DatabaseTable<User> = db.table("users");

// Type-safe import with automatic conversion (Importable::import takes &mut self)
db_users.import(csv_users).await?;
```

`Importable` is only a trait at this stage: no dataset in this repository implements it yet, so ID
mapping, type conversion and batching are left to the implementation.

## Integration with Vantage Framework

Dataset traits form the foundation for higher-level Vantage components:

- **vantage-table**: Builds structured tables with schema validation on top of Dataset traits
- **vantage-vista**: The type-erased `Vista` handle implements the `ValueSet` family
- **vantage-expressions**: Query building that targets datasets as data sources
- **UI adapters**: Generic table components that work with any Dataset implementation

This layered approach enables building complex data applications while maintaining clean separation
of concerns and maximum reusability across different storage backends.

## Error Handling

All traits use `vantage_core::Result<T>` and `VantageError` for consistent error handling across the
Vantage ecosystem:

```rust,ignore
use vantage_core::{Context, Result};

// All trait methods return Result<T>
async fn get_user(&self, id: &str) -> Result<Option<User>> {
    self.get(id).await
        .context("Failed to retrieve user")
}
```

This ensures errors can be properly propagated and handled in applications using multiple Vantage
components.
