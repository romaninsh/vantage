# Model-Driven Architecture

Vantage is opinionated about how you structure business software. The framework prescribes a
**Model-Driven Architecture** (MDA) where entities, relationships, and business rules live in a
shared model crate — decoupled from any specific persistence, UI, or API layer.

<!-- toc -->

---

## The idea

Most Rust projects scatter database queries across handlers, services, and utilities. When the
schema changes or a new backend is needed, you're hunting through dozens of files.

Vantage inverts this. You define your entities **once** in a model crate, then every consumer — REST
API, CLI, desktop UI, background worker — uses the same `Table` and `DataSet` interfaces. The model
is the source of truth.

```mermaid
graph TD
    M[Model Crate] --> API[Axum API]
    M --> CLI[CLI Tool]
    M --> UI[Desktop UI]
    M --> Worker[Background Worker]
    M --> Test[Mock Tests]
    style M fill:#4a7c59,color:#fff
```

---

## Anatomy of a model crate

The `bakery_model3` crate in the Vantage repo demonstrates the pattern. Here's how it's structured:

```text
bakery_model3/
 ├── src/
 │    ├── lib.rs          ← re-exports, DB connection helpers
 │    ├── bakery.rs       ← Bakery entity + table constructors
 │    ├── client.rs       ← Client entity + table constructors
 │    ├── order.rs        ← Order entity + table constructors
 │    └── product.rs      ← Product entity + table constructors
 └── examples/
      ├── cli.rs          ← multi-source CLI using AnyTable
      └── 0-intro.rs      ← direct SurrealDB queries
```

Each entity file follows the same pattern: **struct → trait impls → table constructors**.

---

## Defining entities

An entity is a plain Rust struct. The `#[entity(...)]` macro generates `Record` conversions for each
persistence's type system:

```rust
#[entity(CsvType, SurrealType, SqliteType, PostgresType, MongoType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Client {
    pub name: String,
    pub email: String,
    pub contact_details: String,
    pub is_paying_client: bool,
    pub bakery_id: Option<String>,
}
```

One struct, five type systems. The same `Client` works with CSV files, SurrealDB, SQLite, Postgres,
and MongoDB — each using its own native `Record<AnyType>` representation.

```admonish tip title="No id field in the struct"
The entity struct does **not** include an `id` field. IDs are managed by the table via
`with_id_column()` — keeping the entity focused on business data.
```

---

## Table constructors

Each entity provides table constructors per persistence. These are plain functions that return
`Table<DB, Entity>` with columns, relationships, and computed fields pre-configured:

```rust
impl Client {
    pub fn csv_table(csv: Csv) -> Table<Csv, Client> {
        Table::new("client", csv)
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", Bakery::csv_table)
            .with_many("orders", "client_id", Order::csv_table)
    }

    pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Client> {
        Table::new("client", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<bool>("is_paying_client")
            .with_one("bakery", "bakery", Bakery::surreal_table)
            .with_many("orders", "client", Order::surreal_table)
            .with_expression("order_count", |t| {
                let orders = t.get_subquery_as::<Order>("orders").unwrap();
                orders.get_count_query()
            })
    }

    pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Client> {
        Table::new("client", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", Bakery::sqlite_table)
            .with_many("orders", "client_id", Order::sqlite_table)
    }
}
```

Notice how the shape is consistent but details differ — SurrealDB uses embedded document references
(`"bakery"` not `"bakery_id"`), and only SurrealDB gets the computed `order_count` expression field
(which requires correlated subquery support).

---

## Relationships

Relationships are declared on the table, not the entity. Two methods:

- **`with_one("name", "fk_field", constructor)`** — foreign key to parent (many-to-one)
- **`with_many("name", "fk_field", constructor)`** — parent to children (one-to-many)

Traversal is synchronous and returns a new `Table` with conditions applied:

```rust
let paying = Client::surreal_table(db)
    .with_condition(clients["is_paying_client"].eq(true));

// Traverse — returns Table<SurrealDB, Order> with subquery condition
let orders = paying.get_ref_as::<SurrealDB, Order>("orders").unwrap();

// The generated query filters orders by paying clients automatically
for (id, order) in orders.list().await? {
    println!("{}: {}", id, order.total);
}
```

---

## Computed fields

`with_expression` adds fields that don't exist in the database — they're computed via correlated
subqueries:

```rust
.with_expression("order_count", |t| {
    let orders = t.get_subquery_as::<Order>("orders").unwrap();
    orders.get_count_query()
})
// SELECT *, (SELECT COUNT(*) FROM order WHERE order.client = client.id)
//   AS order_count FROM client
```

These fields appear in `ReadableValueSet` results alongside physical columns.

---

## Connection management

The model crate owns database connections. `bakery_model3` uses a `OnceLock<SurrealDB>` pattern for
global access, with a DSN-based connection function:

```rust
pub async fn connect_surrealdb() -> Result<()> {
    let dsn = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".into());

    let client = SurrealConnection::dsn(&dsn)?
        .connect().await?;

    set_surrealdb(SurrealDB::new(client))
}
```

Consumers call `connect_surrealdb()` once at startup, then use `Client::surreal_table(surrealdb())`
anywhere.

---

## Using the model

Once the model crate exists, consumers are simple. They don't know or care about SQL, SurrealQL, or
BSON — they work with entities and tables:

```rust
// In an Axum handler
async fn list_clients() -> Json<Vec<Client>> {
    let clients = Client::surreal_table(surrealdb())
        .with_condition(clients["is_paying_client"].eq(true));
    Json(clients.list().await.unwrap().into_values().collect())
}

// In a CLI tool
let table = Client::sqlite_table(db);
println!("{} clients", table.get_count().await?);

// In a test — no database needed
let mock = MockTableSource::new()
    .with_data("client", test_data).await;
let table = Table::<MockTableSource, Client>::new("client", mock);
assert_eq!(table.get_count().await?, 3);
```

```admonish info title="Multi-persistence models"
The same model crate can expose table constructors for multiple persistences. Your production
code uses Postgres, your CLI reads CSV exports, your tests use mocks — all sharing the same
entity definitions and business rules.
```

---

## Type-erased access

For truly generic code (UI grids, admin panels, config-driven tools), wrap tables with `AnyTable`:

```rust
let tables: Vec<AnyTable> = vec![
    AnyTable::from_table(Client::surreal_table(db.clone())),
    AnyTable::from_table(Product::sqlite_table(sqlite.clone())),
    AnyTable::from_table(Order::csv_table(csv.clone())),
];

// Same code handles all three — different databases, same interface
for table in &tables {
    println!("{}: {} records", table.table_name(), table.get_count().await?);
}
```

See also: `bakery_model3/examples/cli.rs` for a complete multi-source CLI built on this pattern.

---

## The layered architecture

Putting it all together, Vantage prescribes four layers:

```text
┌─────────────────────────────────────────────────┐
│  4. Consumers                                   │
│     Axum API · CLI · egui · Tauri · gRPC        │
├─────────────────────────────────────────────────┤
│  3. Business Logic                              │
│     Traits on Table · custom methods · rules    │
├─────────────────────────────────────────────────┤
│  2. Model Crate                                 │
│     Entities · Table constructors · Relations   │
├─────────────────────────────────────────────────┤
│  1. Persistence                                 │
│     SurrealDB · Postgres · SQLite · CSV · API   │
└─────────────────────────────────────────────────┘
```

Layer 1 is implemented once per database (or use an existing Vantage crate). Layer 2 is your model
crate — entity definitions, relationships, computed fields. Layer 3 adds business-specific traits
and methods on top of `Table`. Layer 4 is any number of consumers that import the model and don't
think about persistence.

```admonish success title="Why this works"
The model is the only place that knows about database structure. Business logic works against
abstract `Table` and `DataSet` interfaces. Consumers work against business logic traits. Change
the database — only layer 1 and 2 change. Add a new UI — only layer 4 changes. The architecture
scales to hundreds of entities and dozens of developers without coupling.
```
