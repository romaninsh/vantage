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
 │    ├── animal.rs       ← a custom column type
 │    ├── bakery.rs       ← Bakery entity + table constructors
 │    ├── client.rs       ← Client entity + table constructors
 │    ├── order.rs        ← Order entity + table constructors
 │    └── product.rs      ← Product entity + table constructors
 └── examples/
      ├── 0-intro.rs              ← conditions and traversal over CSV
      ├── cli-vista.rs            ← multi-source CLI using Vista
      ├── contained-surreal.rs    ← embedded order lines on SurrealDB
      ├── dynamo-smoke.rs         ← the same model on DynamoDB
      └── implicit-references.rs  ← dotted columns
```

Each entity file follows the same pattern: **struct → trait impls → table constructors**.

---

## Defining entities

An entity is a plain Rust struct. The `#[entity(...)]` macro generates `Record` conversions for each
persistence's type system:

```rust
# use vantage_csv::AnyCsvType;
# use vantage_mongodb::AnyMongoType;
# use vantage_sql::postgres::AnyPostgresType;
# use vantage_sql::sqlite::AnySqliteType;
# use vantage_surrealdb::types::AnySurrealType;
# use vantage_types::entity;
#
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
and MongoDB — each using its own native `Record<AnyType>` representation. The macro needs each
`Any*Type` in scope, which is why a model file imports them from the driver crates.

```admonish tip title="No id field in the struct"
The entity struct does **not** include an `id` field. IDs are managed by the table via
`with_id_column()` — keeping the entity focused on business data.
```

---

## Table constructors

Each entity provides table constructors per persistence. These are plain functions that return
`Table<DB, Entity>` with columns, relationships, and computed fields pre-configured:

```rust
# use vantage_csv::{AnyCsvType, Csv};
# use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
# use vantage_surrealdb::surrealdb::SurrealDB;
# use vantage_surrealdb::thing::Thing;
# use vantage_surrealdb::types::AnySurrealType;
# use vantage_table::table::Table;
# use vantage_types::entity;
#
# #[entity(CsvType, SurrealType, SqliteType)]
# #[derive(Debug, Clone, PartialEq, Default)]
# pub struct Bakery {
#     pub name: String,
# }
#
# impl Bakery {
#     pub fn csv_table(csv: Csv) -> Table<Csv, Bakery> {
#         Table::new("bakery", csv).with_column_of::<String>("name")
#     }
#     pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Bakery> {
#         Table::new("bakery", db).with_id_column("id").with_column_of::<String>("name")
#     }
#     pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Bakery> {
#         Table::new("bakery", db).with_id_column("id").with_column_of::<String>("name")
#     }
# }
#
# #[entity(CsvType, SurrealType, SqliteType)]
# #[derive(Debug, Clone, PartialEq, Default)]
# pub struct Order {
#     pub is_deleted: bool,
# }
#
# impl Order {
#     pub fn csv_table(csv: Csv) -> Table<Csv, Order> {
#         Table::new("order", csv)
#             .with_column_of::<String>("client_id")
#             .with_column_of::<bool>("is_deleted")
#     }
#     pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Order> {
#         Table::new("order", db)
#             .with_id_column("id")
#             .with_column_of::<Thing>("client")
#             .with_column_of::<bool>("is_deleted")
#     }
#     pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Order> {
#         Table::new("client_order", db)
#             .with_id_column("id")
#             .with_column_of::<String>("client_id")
#             .with_column_of::<bool>("is_deleted")
#     }
# }
#
# #[entity(CsvType, SurrealType, SqliteType)]
# #[derive(Debug, Clone, PartialEq, Default)]
# pub struct Client {
#     pub name: String,
#     pub email: String,
#     pub is_paying_client: bool,
# }
#
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
(`"bakery"` not `"bakery_id"`), and in this excerpt only SurrealDB gets the computed `order_count`
expression field. The SQL backends support it too (the real `client.rs` declares it on SQLite and
Postgres); CSV can't, because it has no correlated subqueries to lower it to.

---

## Relationships

Relationships are declared on the table, not the entity. Two methods:

- **`with_one("name", "fk_field", constructor)`** — foreign key to parent (many-to-one)
- **`with_many("name", "fk_field", constructor)`** — parent to children (one-to-many)

Traversal is synchronous and returns a new `Table` with conditions applied. Reading the result
needs a populated database, so this block is compiled but not run:

```rust,no_run
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, PartialEq, Default)]
# pub struct Client {
#     pub name: String,
#     pub is_paying_client: bool,
# }
#
# impl Client {
#     pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Client> {
#         Table::new("client", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<bool>("is_paying_client")
#             .with_many("orders", "client_id", Order::sqlite_table)
#     }
# }
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, PartialEq, Default)]
# pub struct Order {
#     pub client_id: Option<String>,
#     pub is_deleted: bool,
# }
#
# impl Order {
#     pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Order> {
#         Table::new("client_order", db)
#             .with_id_column("id")
#             .with_column_of::<String>("client_id")
#             .with_column_of::<bool>("is_deleted")
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:bakery.sqlite").await.context("connect")?;
let mut paying = Client::sqlite_table(db);
paying.add_condition(paying["is_paying_client"].eq(true));

// Traverse — returns Table<SqliteDB, Order> with a subquery condition
let orders = paying.get_ref_as::<Order>("orders")?;

// The generated query filters orders by paying clients automatically
for (id, order) in orders.list().await? {
    println!("{id}: client {:?}", order.client_id);
}
# Ok(())
# }
```

---

## Computed fields

`with_expression` adds fields that don't exist in the database — they're computed via correlated
subqueries:

```rust
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, PartialEq, Default)]
# pub struct Order {
#     pub client_id: Option<String>,
# }
#
# impl Order {
#     pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Order> {
#         Table::new("client_order", db)
#             .with_id_column("id")
#             .with_column_of::<String>("client_id")
#     }
# }
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, PartialEq, Default)]
# pub struct Client {
#     pub name: String,
# }
#
# pub fn client_table(db: SqliteDB) -> Table<SqliteDB, Client> {
#     Table::new("client", db)
#         .with_id_column("id")
#         .with_column_of::<String>("name")
#         .with_many("orders", "client_id", Order::sqlite_table)
.with_expression("order_count", |t| {
    let orders = t.get_subquery_as::<Order>("orders").unwrap();
    orders.get_count_query()
})
// SELECT *, (SELECT COUNT(*) FROM client_order WHERE client_order.client_id = client.id)
//   AS order_count FROM client
# }
```

These fields appear in `ReadableValueSet` results alongside physical columns.

---

## Implicit references — dotted columns

Surfacing a related row's field is common enough that it has a declarative form. A **dotted name** in
`with_active_columns` traverses declared `has_one` relations and imports the target's field as a
read-only column, aliased under the literal dotted name — no hand-written expression:

```rust
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, PartialEq, Default)]
# pub struct Bakery {
#     pub name: String,
# }
#
# impl Bakery {
#     pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Bakery> {
#         Table::new("bakery", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#     }
# }
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, PartialEq, Default)]
# pub struct Client {
#     pub name: String,
# }
#
# impl Client {
#     pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Client> {
#         Table::new("client", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<String>("bakery_id")
#             .with_one("bakery", "bakery_id", Bakery::sqlite_table)
#     }
# }
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, PartialEq, Default)]
# pub struct Order {
#     pub client_id: Option<String>,
# }
#
# impl Order {
#     pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Order> {
#         Table::new("client_order", db)
#             .with_id_column("id")
#             .with_column_of::<String>("client_id")
#             .with_one("client", "client_id", Client::sqlite_table)
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite::memory:").await.context("connect")?;
let orders = Order::sqlite_table(db)
    .with_active_columns(&["id", "client.name", "client.bakery.name"])?;
// SELECT id,
//   (SELECT name FROM client WHERE client.id = client_order.client_id) AS "client.name",
//   (SELECT (SELECT name FROM bakery WHERE bakery.id = client.bakery_id)
//      FROM client WHERE client.id = client_order.client_id)           AS "client.bakery.name"
// FROM client_order
# let _ = orders;
# Ok(())
# }
```

`client.name` is one hop; `client.bakery.name` recurses through two. Each backend lowers the
traversal into its own query — SQL nests correlated scalar subqueries, SurrealDB emits a native idiom
path (`client.name`, each segment escaped separately), and MongoDB adds `$lookup` stages. The row
comes back with a **flat key** equal to the dotted name (`row.get("client.bakery.name")`).

A plain (non-dotted) entry simply restricts projection to that column — `with_active_columns` is also
how you narrow a wide table to the columns you actually read. Everything is validated when the table
is built, so mistakes surface immediately rather than at fetch time:

- an unknown column or relation is a **build-time error**;
- a `has_many` hop is rejected (traversal is `has_one`-only — a to-many field is a set, not a value);
- a backend that can lower neither a subquery, an idiom path nor a join (e.g. CSV, REST) refuses
  dotted names up front; same-datasource only.

Imported columns are read-only: they are flagged `calculated` for consumers, stripped from
full-record write payloads (insert, replace, generated-id insert) so a read-modify-save round-trip
never tries to persist `client.name` as a real field, and rejected outright in a `patch` — a partial
payload naming a read-only column is explicit intent, and silently dropping it would turn the patch
into a successful no-op.
This is the declarative counterpart to the manual `with_expression` + `get_subquery_as` recipe above —
reach for `with_expression` when you need an arbitrary expression, and a dotted column when you just
want a related field. The full treatment — traversal forms, validation, write semantics, per-backend
lowering, and how relations surface on Vistas and Dio — is the
[Relations & Traversal](relations.md) guide.

---

## Connection management

The model crate owns database connections. `bakery_model3` keeps its SurrealDB handle in a
`OnceLock`, set once by a DSN-based connection function. The same pattern for SQLite, which needs
a database file, so the block is compiled but not run:

```rust,no_run
# use std::sync::OnceLock;
#
# use vantage_core::{Context, Result, error};
# use vantage_sql::sqlite::SqliteDB;
#
static SQLITE: OnceLock<SqliteDB> = OnceLock::new();

pub async fn connect_sqlite() -> Result<()> {
    let dsn = std::env::var("SQLITE_URL").unwrap_or_else(|_| "sqlite:bakery.sqlite".into());
    let db = SqliteDB::connect(&dsn).await.context("Failed to connect to SQLite")?;
    SQLITE.set(db).map_err(|_| error!("SQLite connection already set"))
}

pub fn sqlite() -> SqliteDB {
    SQLITE.get().expect("call connect_sqlite() first").clone()
}
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<()> {
# connect_sqlite().await?;
# let _ = sqlite();
# Ok(())
# }
```

Consumers call `connect_sqlite()` once at startup, then use `Client::sqlite_table(sqlite())`
anywhere.

---

## Using the model

Once the model crate exists, consumers are simple. They don't know or care about SQL, SurrealQL, or
BSON — they work with entities and tables. The handler and the CLI read a database, so the block is
compiled but not run:

```rust,no_run
# use std::sync::OnceLock;
#
# use axum::Json;
# use serde::Serialize;
# use vantage_memory::{AnyMemoryType, MemoryDB};
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
#
# #[entity(SqliteType, MemoryType)]
# #[derive(Debug, Clone, PartialEq, Default, Serialize)]
# pub struct Client {
#     pub name: String,
#     pub is_paying_client: bool,
# }
#
# impl Client {
#     pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Client> {
#         Table::new("client", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<bool>("is_paying_client")
#     }
#
#     pub fn memory_table(db: MemoryDB) -> Table<MemoryDB, Client> {
#         Table::new("client", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<bool>("is_paying_client")
#     }
# }
#
# static SQLITE: OnceLock<SqliteDB> = OnceLock::new();
#
# fn sqlite() -> SqliteDB {
#     SQLITE.get().expect("connected").clone()
# }
#
// In an Axum handler
async fn list_clients() -> Json<Vec<Client>> {
    let mut clients = Client::sqlite_table(sqlite());
    clients.add_condition(clients["is_paying_client"].eq(true));
    Json(clients.list().await.unwrap().into_values().collect())
}

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let _ = list_clients;
// In a CLI tool
let table = Client::sqlite_table(sqlite());
println!("{} clients", table.get_count().await?);

// In a test — no database needed
let table = Client::memory_table(MemoryDB::new());
table.insert_return_id(&Client { name: "Marty".into(), is_paying_client: true }).await?;
assert_eq!(table.list().await?.len(), 1);
# Ok(())
# }
```

The test reaches for `vantage-memory`, an in-process store with a typed `TableSource`. The model
gives it one more constructor and one more type system in `#[entity(..)]`; nothing else about
`Client` changes.

```admonish info title="Multi-persistence models"
The same model crate can expose table constructors for multiple persistences. Your production
code uses Postgres, your CLI reads CSV exports, your tests use mocks — all sharing the same
entity definitions and business rules.
```

---

## Type-erased access

For truly generic code (UI grids, admin panels, config-driven tools), wrap tables in a
[`Vista`](./new-persistence/step8-vista-integration.md) — the schema-bearing handle that replaced
the removed `AnyTable` in 0.5. Each driver's factory wraps its own tables, and every result is the
same type. Listing reads real data, so the block is compiled but not run:

```rust,no_run
# use vantage_csv::{AnyCsvType, Csv};
# use vantage_dataset::ReadableValueSet;
# use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
# use vantage_table::table::Table;
# use vantage_types::entity;
# use vantage_core::Context;
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# pub struct Product {
#     pub name: String,
# }
#
# impl Product {
#     pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db).with_id_column("id").with_column_of::<String>("name")
#     }
# }
#
# #[entity(CsvType)]
# #[derive(Debug, Clone, Default)]
# pub struct Order {
#     pub client_id: String,
# }
#
# impl Order {
#     pub fn csv_table(csv: Csv) -> Table<Csv, Order> {
#         Table::new("order", csv).with_column_of::<String>("client_id")
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let sqlite = SqliteDB::connect("sqlite:bakery.sqlite").await.context("connect")?;
# let csv = Csv::new("bakery_model3/data");
let vistas = vec![
    sqlite.vista_factory().from_table(Product::sqlite_table(sqlite.clone()))?,
    csv.vista_factory().from_table(Order::csv_table(csv.clone()))?,
];

// Same code handles both — different backends, same interface
for v in &vistas {
    println!("{}: {} records", v.name(), v.list_values().await?.len());
}
# Ok(())
# }
```

See also: `bakery_model3/examples/cli-vista.rs` for a complete multi-source CLI built on this
pattern.

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
