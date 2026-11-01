# Vantage

[![Book](https://github.com/romaninsh/vantage/actions/workflows/book.yaml/badge.svg)](https://vantage-ui.com/book/)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/romaninsh/vantage)

Vantage is a data framework for Rust. You describe your business entities (Client, Order, Invoice)
once, as Rust types, and use them against SQL databases, SurrealDB, MongoDB, REST and GraphQL APIs,
AWS, Kubernetes, CSV files or memory through one API.

Vantage works with **sets of records**, not single rows. A table with a condition is a smaller set.
Traversing a relation turns one set into another. Counting, filtering and deleting run in the
database whenever the database can do it, so the records never pass through your process.

There are three layers, and you can stop at any of them:

- **Table**: typed entities, conditions, relations, expressions and CRUD, checked at compile time.
- **Vista**: the same table with the types erased. It carries its own schema, can be loaded from
  YAML, and is what generic code (a CLI, an admin UI, an HTTP API) works with.
- **Diorama**: a cache and live view over a Vista, for user interfaces. It loads the rows on screen,
  keeps them current and applies edits optimistically.

## Looking for the app? Meet Vantage UI

If you want a finished product rather than a library, start with
**[Vantage UI](https://vantage-ui.com/)**: a native, local-first admin console that you configure by
talking to an AI coding agent. Point it at your databases, APIs, cloud accounts or CLI tools and
describe the tool you need. The agent writes the configuration (YAML for structure, Rhai scripts for
logic) into a folder you own, and the app draws tables, forms, dashboards and dialogs on top of your
data.

Vantage UI is built on the crates in this repository. It is a free download for macOS. If you'd
rather build your own service, CLI or UI, the rest of this README is for you.

## Example

An entity and its table, from the [book's second chapter](https://vantage-ui.com/book/intro/step2-tables.html):

```rust
use vantage_sql::prelude::*;
use vantage_types::prelude::*;

#[entity(SqliteType)]
#[derive(Clone, Default)]
pub struct Product {
    pub name: String,
    pub price: i64,
}

impl Product {
    pub fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
        let is_deleted = Column::<bool>::new("is_deleted");
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
            .with_condition(is_deleted.eq(false))
    }
}
```

The table definition lives in your model crate. Business code uses it without knowing where the data
is stored:

```rust
let table = Product::table(db);

let all = table.list().await?;             // IndexMap<String, Product>
let pie = table.get("4").await?;           // Option<Product>

let muffin = Product { name: "Muffin".into(), price: 175 };
table.insert(&"8".to_string(), &muffin).await?;
table.delete(&"8".to_string()).await?;
```

Relations work on sets too. A category table declares
`.with_many("products", "category_id", Product::table)`, and then:

```rust
let products = Category::table(db)
    .with_search("t")
    .get_ref_as::<Product>("products")?;
```

That is one query, with a subquery inside it:

```sql
SELECT ... FROM "product"
WHERE "category_id" IN (SELECT "id" FROM "category" WHERE "name" LIKE '%t%')
AND "is_deleted" = 0
```

Writes are safe to retry. Inserting an id that already exists returns the stored row, replacing a
row with the same values changes nothing, and deleting a row that is already gone succeeds. A write
through a narrowed table can't touch rows outside its conditions.

## Vista

Every driver ships a factory that turns a typed table into a `Vista`:

```rust
let table = Product::table(db.clone());
let vista = SqliteVistaFactory::new(db).from_table(table)?;

let mut v = vista.clone();
v.add_search("tart")?;
v.add_order("price", SortDirection::Descending)?;
let rows = v.list_values().await?;
```

A Vista has no generic parameters. Values are CBOR (`ciborium::Value`), so integers, decimals,
datetimes and record ids keep their type. The Vista knows its columns, id column, title columns and
references, and it declares what its driver can do (`can_order`, `can_search`, `can_fetch_page`,
`can_subscribe` and so on). Asking for something the driver can't do returns an `Unsupported` error
instead of a silently unfiltered result.

A Vista doesn't have to come from Rust code. Each factory also loads one from YAML:

```yaml
name: category
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: category_id
```

When YAML isn't enough, a [Rhai](https://rhai.rs) script fills the gap. Here a Vista is defined by a
query:

```yaml
name: expensive_products
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
sqlite:
  rhai: |
    select().from("product").field("id").field("name").where(expr("price > 150"))
```

`vantage-rhai` runs every script in a YAML file, with the same data vocabulary on every database.
`VistaCatalog` in `vantage-vista-factory` follows references that cross databases, for example from a
Postgres table into a MongoDB collection.

## Diorama

`vantage-diorama` puts a cache and live updates between a Vista and a user interface:

```rust
let products = lens.make_dio(products_vista).await?;

// Use it like any other Vista, but reads are served from the cache:
let mut facade = products.vista();
facade.add_condition_eq("category", "books".into())?;
let books = facade.list_values().await?;

// Or open a view that updates when the data changes:
let scenery = products
    .table_scenery()
    .sort("price", SortDir::Asc)
    .open()
    .await?;
```

A scenery loads only the rows in its viewport and refreshes without clearing what's on screen. It
polls less when nobody is looking and applies edits optimistically, rolling them back when the
backend refuses. Views over the same query share one fetch. Diorama can also enrich rows with
details from another source (augmentation), and `vantage-diorama-aggregate` keeps counts, sums and
group-bys current as the data changes.

## Datasources

| Crate                 | Backend                                         | Version | Writes     | Stays current by                                  |
| --------------------- | ----------------------------------------------- | ------- | ---------- | ------------------------------------------------- |
| `vantage-sql`         | PostgreSQL, MySQL/MariaDB, SQLite               | 0.8     | yes        | Postgres: `LISTEN/NOTIFY` (opt-in); others: poll  |
| `vantage-surrealdb`   | SurrealDB (over `surreal-client`)               | 1.0     | yes        | `LIVE SELECT`                                     |
| `vantage-mongodb`     | MongoDB                                         | 1.1     | yes        | change streams on a replica set, else poll        |
| `vantage-memory`      | in-process tables                               | 1.2     | yes        | in-process events                                 |
| `vantage-redb`        | redb embedded key-value store                   | 0.7     | yes        | typed tables only (no Vista yet)                  |
| `vantage-api-client`  | REST and GraphQL APIs                           | 1.0     | no         | poll                                              |
| `vantage-csv`         | CSV files                                       | 0.6     | no         | poll                                              |
| `vantage-aws`         | AWS APIs (Logs, ECS, IAM, S3, Lambda, DynamoDB) | 0.7     | no         | poll                                              |
| `vantage-kubernetes`  | Kubernetes API                                  | 0.6     | no         | poll                                              |
| `vantage-cmd`         | local CLI tools (`aws`, `kubectl`, `gh`)        | 0.6     | no         | poll                                              |
| `vantage-spacetimedb` | SpacetimeDB (not yet on crates.io)              | 0.6     | owner only | subscription                                      |

The [Live Data chapter](https://vantage-ui.com/book/live-data.html) explains what each backend does
to stay current and what setup it needs.

The SQL crate also has query builders for each dialect (`SqliteSelect`, `PostgresSelect`,
`MysqlSelect`) with joins, CTEs, window functions and vendor-specific functions. The SurrealDB crate
has graph traversal. REST and GraphQL requests go through `vantage-api-pool`'s `ResilientClient`, which
handles parallelism limits, rate limits, retries, a circuit breaker and auth refresh.

`vantage-faker` generates seeded, related test data into `vantage-memory`, and can keep changing it
with Rhai "sims" or make a source slow and unreliable on purpose, for testing UIs.

## The crates

Foundation, all at 1.0 or later:

- [`vantage-core`](vantage-core): `VantageError` with context, error kinds and request priority.
- [`vantage-types`](vantage-types/README.md): per-database type systems (`vantage_type_system!`),
  `Record`, and the `#[entity]` macro.
- [`vantage-expressions`](vantage-expressions/README.md): query templates with parameters, deferred
  values that resolve at execution time, and the `Selectable` query builder trait.
- [`vantage-dataset`](vantage-dataset/README.md): read, write and insert traits for values and
  entities, active records, and an in-memory implementation.
- [`vantage-table`](vantage-table/README.md): `Table<T, E>` with columns, conditions, relations,
  expressions, aggregates and lifecycle hooks.
- [`vantage-vista`](vantage-vista/README.md): the type-erased `Vista` and the `TableShell` trait
  drivers implement.
- [`vantage-rhai`](vantage-rhai/README.md): one Rhai host for every YAML script slot.

Live data and tooling:

- [`vantage-diorama`](vantage-diorama/README.md) (0.14) and `vantage-diorama-aggregate`: cached,
  live views for UIs.
- `vantage-vista-factory`: `VistaCatalog`, for references across databases.
- [`vantage-inventory`](vantage-inventory/README.md): loads a folder of YAML files into typed
  values, with `!include`, JSON-schema validation and file watching.
- `vantage-api-adapters`: axum routers that serve a Dio over HTTP, with watch streams, and mount
  actions as POST routes.
- `vantage-action` and `vantage-bundle`: business operations defined once on a model, and the
  contract a compiled-in model crate implements.
- `vantage-cli-util`: terminal tables, JSON/NDJSON/CBOR output and a generic Vista CLI.
- `vantage-log-writer`: append-only JSONL tables.

## Installation

```toml
[dependencies]
vantage-core = "2"
vantage-types = "2"
vantage-dataset = "2"
vantage-table = "4"
vantage-sql = { version = "0.11", features = ["sqlite"] }  # also "postgres", "mysql", "vista", "rhai"
tokio = { version = "1", features = ["full"] }
```

Add the other drivers you need (`vantage-surrealdb`, `vantage-mongodb`, `vantage-api-client`, ...)
and turn on their `vista` feature to use them through a Vista. The crates use edition 2024.

## Learning

- [The book](https://vantage-ui.com/book/) starts with the SQLite query builder and builds up to
  typed tables, an axum server and Vista. It then splits into two paths: a cached facade over a
  custom API (S3), or a live API over SQL that moves from SQLite to Postgres with `LISTEN/NOTIFY`
  and then to SurrealDB. Reference chapters cover expressions, relations, Rhai, live data and
  adding a new datasource.
- `learn-1` to `learn-11` are the runnable projects behind those chapters.
- `bakery_model3` is a shared sample model with constructors for several backends, and
  `bakery_model4` defines the same model as YAML Vistas.
- The `example_*` crates show the model in egui, GPUI, Slint, Tauri, ratatui and Cursive.

## For AI agents

- The book as markdown: [llms.txt](https://vantage-ui.com/book/llms.txt), or the whole book in
  [llms-full.txt](https://vantage-ui.com/book/llms-full.txt)
- This repo as an MCP server, via [GitMCP](https://gitmcp.io):

  ```sh
  claude mcp add --transport http vantage-docs https://gitmcp.io/romaninsh/vantage
  ```

- Questions about the code: [DeepWiki](https://deepwiki.com/romaninsh/vantage)

## Status

Each crate has its own version and changelog. The crates at 1.0 or later follow semver. The pre-1.0
crates (`vantage-sql`, `vantage-diorama` and the tooling crates) may still change between minor
versions. `vantage-aws`, `vantage-kubernetes`, `vantage-cmd` and `vantage-spacetimedb` are
incubating: read-only or partial, and their APIs are still moving.

## Author

Vantage is written by **Romans Malinovskis**. To get in touch:

- <https://www.linkedin.com/in/romansmalinovskis>
- <https://bsky.app/profile/nearly.guru>
