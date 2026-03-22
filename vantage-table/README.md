# Vantage Table

Type-safe table abstractions with ActiveEntity support for database-agnostic CRUD operations.

## Quick Start

```rust
use vantage_dataset::prelude::*;
use vantage_table::table::Table;

#[derive(Debug, Clone)]
struct User {
    name: String,
    email: String,
    active: bool,
}

// Create table with any compatible data source
let table = Table::new("users", data_source)
    .with_column_of::<String>("name")
    .with_column_of::<String>("email")
    .with_column_of::<bool>("active")
    .into_entity::<User>();

// Load and modify entities
let mut user = table.get_entity(&"user123".to_string()).await?
    .unwrap_or_else(|| table.new_entity("user123".to_string(), User {
        name: "New User".to_string(),
        email: "user@example.com".to_string(),
        active: false,
    }));

// Direct field modification
user.active = true;
user.email = "updated@example.com".to_string();

// Automatic persistence
user.save().await?;
```

## Features

- **ActiveEntity Pattern**: Load entities, modify fields directly, and save changes automatically
- **Database Agnostic**: Works with CSV, SurrealDB, and custom data sources
- **Type Safety**: Full compile-time validation of entity structure and field access
- **Flexible Queries**: Integration with vantage-expressions for complex query building

## Core Operations

### Columns

Define table structure with typed columns for precise querying:

```rust
// Define columns with types
let users_table = Table::new("users", data_source)
    .with_column_of::<String>("name")
    .with_column_of::<i64>("age")
    .with_column_of::<bool>("active")
    .into_entity::<User>();

// Add conditions to filter records
let active_users = users_table
    .with_condition(users_table["active"].eq(true));

// Work with filtered dataset
for mut user in active_users.list_entities().await? {
    user.last_login = Utc::now();
    user.save().await?;
}
```

> **Note:** Comparison operators like `.gt()`, `.lt()` are planned but not yet implemented.
> Currently `.eq()` is the primary condition operator.

### Loading Records

There are multiple ways to load records depending on your needs:

- **`get_entity(id)`** - Load ActiveEntity by ID (returns `None` if not found)
- **`get_value(id)`** - Load raw record data by ID
- **`list()`** - Load all raw entities as `IndexMap<Id, Entity>`
- **`list_entities()`** - Load all entities as `Vec<ActiveEntity>`
- **`list_values()`** - Load all raw record data

```rust
// Load raw entities (no save functionality)
let users: IndexMap<String, User> = table.list().await?;
for (id, user) in users {
    println!("User {}: {} ({})", id, user.name, user.email);
}

// Load raw record data for inspection
if let Ok(record) = table.get_value(&"user123".to_string()).await {
    println!("Raw data: {:?}", record);
    println!("Name field: {:?}", record["name"]);
}
```

### Get-or-Create Pattern

```rust
// Get existing or create new entity
let mut user = table.get_entity(&"user123".to_string()).await?
    .unwrap_or_else(|| table.new_entity("user123".to_string(), User::default()));
```

### Table Metadata

Various ways to interact with table metadata and statistics:

```rust
// Get record count immediately
let count: i64 = table.get_count().await?;
println!("Total users: {}", count);

// Get count as AssociatedExpression for use in other queries
let count_expr = table.get_table_expr_count();
let result = count_expr.get().await?; // Execute now
// Or use in another expression: expr!("SELECT {} as user_count", count_expr)

// Get aggregated values like max, min, sum (returns AssociatedExpression)
let max_age = table.get_table_expr_max(&table["age"]);
let max_value = max_age.get().await?;

// Get select query builder implementing Selectable trait for complex operations
let select = table.get_select_query();
let results = select
    .with_field("name")
    .with_condition(expr!("active = true"))
    .with_limit(10)
    .execute().await?;
```

See [`AssociatedExpression`] for deferred query execution and [`Selectable`] trait for query builder
interface.

## Diverse Data Persistence Support

Vantage Table works with a wide range of data sources through modular adapter crates:

### Ready-to-Use Adapters

- **vantage-csv**: CSV file backend with type-safe column parsing
- **vantage-surrealdb**: SurrealDB integration with native query building
- **[`MockTableSource`]**: In-memory testing and development mock

### Planned Adapters

- **vantage-sql**: PostgreSQL, MySQL, SQLite support via SQL generation (TODO)

### Create Your Own Vendor Support

Building a custom data source adapter involves:

- **Use vantage-types**: Define accurate persistence type mapping for all popular Rust types
- **Use vantage-expressions**: Design your own query builder primitives supporting nesting and
  cross-database operations
- **Implement [`TableSource`]**: Allow tables to persist in your database
- **Rely on vantage-core**: Unified error handling across the ecosystem
- **Create custom Column types**: Add advanced features specific to your database (or use the
  built-in `Column<T>` which preserves original type info through type-erasure)
- **Add Table extensions**: Support for full-text search, graph operations, or vendor-specific
  features

## Upcoming Features

- **Relationship traversal**: Reference support for navigating between related tables (TODO)
- **Pagination**: Limit/offset support in Table operations (TODO)
- **Comparison operators**: `.gt()`, `.lt()`, `.gte()`, `.lte()` for conditions (TODO)

## Integration

Part of the Vantage framework:

- **vantage-types**: Type system, entity definitions, and `TerminalRender` trait
- **vantage-expressions**: Query building and database abstraction
- **vantage-dataset**: CRUD operation traits
- **vantage-core**: Error handling and utilities
- **vantage-csv**: CSV file data source
- **vantage-cli-util**: Terminal table rendering utilities

## Migration from 0.2

```rust
// Old (0.2): Manual record management
let record = table.get_record(id).await?;
record.set_field("name", "New Name");
table.save_record(record).await?;

// New (0.3): Direct entity modification
let mut entity = table.get_entity(&id).await?.unwrap();
entity.name = "New Name".to_string();
entity.save().await?;
```
