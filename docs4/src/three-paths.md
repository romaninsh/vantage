# Three Paths for Developers

Vantage gives you three ways to work with data. Each path builds on the previous one, and all three
converge into a unified data abstraction layer.

<!-- toc -->

---

```mermaid
graph TD
    EF["① Entity Framework<br/>CRUD over business objects"]
    QB["② Query Building<br/>Vendor-specific SQL power"]
    CP["③ Custom Persistences<br/>APIs, caches, reactive data"]
    EF --> DAL["Universal Data Abstraction Layer"]
    QB --> DAL
    CP --> DAL
    DAL --> E["Export: FFI · API · UI · SDK"]
    style EF fill:#4a7c59,color:#fff
    style QB fill:#2d6a8f,color:#fff
    style CP fill:#8f5a2d,color:#fff
    style DAL fill:#7c2d8f,color:#fff
    style E fill:#555,color:#fff
```

---

## ① Entity Framework

The most common path. Define entities, build tables, use `DataSet` and `ActiveEntity` for CRUD —
your code never touches SQL or any query language. The block reads and writes a populated
database, so it is compiled but not run:

```rust,no_run
# use vantage_dataset::prelude::ActiveEntitySet;
# use vantage_sql::postgres::AnyPostgresType;
# use vantage_sql::prelude::*;
# use vantage_surrealdb::types::AnySurrealType;
# use vantage_types::prelude::*;
#
// Define once
#[entity(SurrealType, SqliteType, PostgresType)]
#[derive(Debug, Clone, Default)]
struct Product {
    name: String,
    price: i64,
    is_deleted: bool,
}
#
# impl Product {
#     fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<bool>("is_deleted")
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:bakery.sqlite").await.context("connect")?;
# let id = "flux_cupcake";

// Use everywhere
let table = Product::sqlite_table(db);
let expensive = table.clone().with_condition(table["price"].gt(200i64));
let count = expensive.get_count().await?;

let mut item = table.get_entity(id).await?.unwrap();
item.price = 350;
item.save().await?;
# let _ = count;
# Ok(())
# }
```

This is the bread and butter of enterprise development — hundreds of entities, each with
relationships, computed fields, and business rules, all persistence-agnostic and testable with
mocks.

```admonish tip title="When to use this path"
You're building business software with well-defined entities (Client, Order, Invoice). You want
clean separation between persistence and logic. You don't need vendor-specific SQL features.
```

---

## ② Query Building

When you need the full power of your database — JOINs, CTEs, window functions, JSONB operators,
array aggregation, recursive queries — drop into the vendor-specific query builder.

### SQLite — CASE, UNION, window functions

```rust
# use vantage_expressions::{Expressive, Order, Selectable};
# use vantage_sql::primitives::alias::AliasExt;
# use vantage_sql::sqlite::sqlite_ident as ident;
# use vantage_sql::sqlite::statements::SqliteSelect;
# use vantage_sql::sqlite_expr;
use vantage_sql::primitives::case::Case;

let select = SqliteSelect::new()
    .with_source("users")
    .with_field("name")
    .with_field("salary")
    .with_expression(
        Case::new()
            .when(sqlite_expr!("{} >= {}", (ident("salary")), 100000.0f64),
                  sqlite_expr!("{}", "senior"))
            .when(sqlite_expr!("{} >= {}", (ident("salary")), 60000.0f64),
                  sqlite_expr!("{}", "mid"))
            .else_(sqlite_expr!("{}", "junior"))
            .as_alias("band"),
    )
    .with_order(ident("salary"), Order::Desc);
# assert!(select.preview().starts_with(r#"SELECT "name", "salary", CASE WHEN"#));
```

### PostgreSQL — DISTINCT ON, LATERAL JOIN, array ops

```rust
# use vantage_expressions::{Expressive, Order, Selectable};
# use vantage_sql::postgres::operation::PostgresOperation;
# use vantage_sql::postgres::pg_ident as ident;
# use vantage_sql::postgres::statements::PostgresSelect;
# use vantage_sql::postgres::statements::select::join::PostgresSelectJoin;
# use vantage_sql::primitives::alias::AliasExt;
let select = PostgresSelect::new()
    .with_distinct_on(ident("user_id").dot_of("o"))
    .with_source_as("orders", "o")
    .with_expression(ident("name").dot_of("u"))
    .with_expression(ident("id").dot_of("o").with_alias("order_id"))
    .with_expression(ident("total").dot_of("o"))
    .with_join(PostgresSelectJoin::inner(
        "users", "u",
        ident("id").dot_of("u").eq(ident("user_id").dot_of("o")),
    ))
    .with_order(ident("user_id").dot_of("o"), Order::Asc)
    .with_order(ident("created_at").dot_of("o"), Order::Desc);
```

Both examples are type-safe — parameters are bound with the correct CBOR type markers, identifiers
are quoted for the target dialect, and results deserialize into typed structs. See the full test
suites:
[SQLite complex queries](https://github.com/romaninsh/vantage/blob/main/vantage-sql/tests/sqlite/3_complex_queries.rs),
[PostgreSQL complex queries](https://github.com/romaninsh/vantage/blob/main/vantage-sql/tests/postgres/3_complex_queries_pg.rs).

```admonish info title="Bridge back into the entity framework"
Complex queries don't have to live in isolation. Wrap them as expression fields
(`with_expression`) or as custom methods on your `Table` — they become part of the entity
framework, available to every consumer of your model crate.
```

```admonish tip title="When to use this path"
You need vendor-specific features — recursive CTEs, window functions, `DISTINCT ON`,
JSONB operators, `generate_series`, array aggregation. You want full control over the query
while keeping type safety and parameterised binding.
```

---

## ③ Custom Persistences

Vantage isn't limited to databases. Implement the persistence traits for anything that stores or
produces data:

- **REST APIs** — `vantage-api-client` wraps any paginated JSON API as a read-only `TableSource`
- **Resilient transport** — `vantage-api-pool` adds retry, a circuit breaker, rate limiting and auth
  refresh to API traffic
- **CSV files** — `vantage-csv` reads structured files with in-memory conditions
- **Local caches** — `vantage-diorama` keeps a local copy (in memory or in a redb file) of any
  slow backend (Postgres, SurrealDB, a REST API) for responsive UIs
- **Message queues** — implement `InsertableDataSet` for append-only sources like Kafka topics
- **Mixed sources** — read from SQL, write through a queue, cache in memory — same `Table` interface

A cache sits above the backend rather than replacing it. Wrap the permanent table as a `Vista`,
put a Dio in front of it, and the Dio hands back the cached copy as another `Vista`. The cache
is filled from a database, so the block is compiled but not run:

```rust,no_run
# use vantage_dataset::ReadableValueSet;
# use vantage_diorama::{Cache, Lens, Loader};
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Product {
#     name: String,
# }
#
# impl Product {
#     fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db).with_id_column("id").with_column_of::<String>("name")
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:bakery.sqlite").await.context("connect")?;
// The permanent backend, type-erased
let products = db.vista_factory().from_table(Product::sqlite_table(db.clone()))?;

// A local cache in front of it
let lens = Lens::builder()
    .cache(Cache::redb("cache.redb"))
    .loader(Loader::eager(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    }))
    .build()?;
let cached = lens.dio(products).build().await?;

// Reads come from the cache; your UI code doesn't care which it holds
let rows = cached.cached().list_values().await?;
# let _ = rows;
# Ok(())
# }
```

```admonish tip title="When to use this path"
You're integrating with external APIs, building offline-first applications, implementing
reactive data layers, or combining multiple data sources behind a single interface. Your custom
persistence becomes a first-class citizen in the entity framework.
```

---

## Convergence

All three paths converge into the same **data abstraction layer**. Entity framework tables, raw
query results, and custom persistence data all flow through `Table`, `DataSet`, and `Vista` —
giving you one interface for the entire organisation's data.

```text
┌──────────────────────────────────────────────────────────────┐
│                  Data Abstraction Layer                       │
│                                                              │
│   Table<SurrealDB, Client>    ← entity framework             │
│   Table<PostgresDB, Report>   ← complex queries              │
│   Table<RestApi, ExternalData> ← custom persistence          │
│   Dio over any Vista          ← cached + reactive            │
│                                                              │
│   Tables implement: DataSet · ValueSet · ActiveEntitySet     │
│   All wrap into: Vista (type-erased, generic code)           │
└──────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
          Axum API        egui/Tauri       FFI / SDK
```

This is where Rust's type system pays off at scale. Every `Table` is checked at compile time. Every
persistence has strict type boundaries. Every consumer works against abstract traits. Add a new
database, API, or cache — nothing else changes. Add a new UI framework or language binding — the
model stays the same.

```admonish success title="The architecture that scales"
Vantage turns your data layer into a universal abstraction. Start with one database and a
handful of entities. Over time, add more persistences, more entities, more consumers. The
architecture doesn't bend — Rust's ownership model and trait system ensure each layer stays
clean, each boundary stays enforced, and each team works independently.
```
