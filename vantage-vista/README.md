# Vantage Vista

`Vista` is the type-erased `Table<T, E>`. It takes a typed table, the kind a model layer defines
(`bakery_model3::Product::sqlite_table()`, `vantage_aws::models::ecs::clusters_table()`), and seals
its types behind a `TableShell`, so generic code (a CLI, a desktop UI, a config-driven dashboard)
can drive any backend through one handle. Rows travel as CBOR (`ciborium::Value`) and ids as
`String`, whatever the backend stores natively.

The shape is the same as `Table`'s: `Vista` is `Clone`, owns its query state, narrows with
`with_*` and changes in place with `add_*`/`set_*`.

## Three kinds of object

Every public type in Vantage above `Table` is one of three kinds, and its API follows from the
kind. A **value** (`Table`, `Vista`, `VistaQuery`, `Reference`) is narrowed by
`clone().with_*()`, which returns a new value and leaves the original alone, and changes through
`add_*`/`set_*` only when you own it (`&mut self`). A **shared handle** (diorama's `Lens` and
`Dio`) never changes after construction: `with_*` derives a new handle. **View state** (diorama's
sceneries and `Servo`) is opened by a builder and changed with `set_*` by its single owner. This
crate holds only values; the other two kinds live in vantage-diorama.

## Examples

### Build a Vista

A driver factory turns a typed table into a Vista:

```rust
use vantage_sql::sqlite::vista::SqliteVistaFactory;

let products = SqliteVistaFactory::new(db.clone()).from_table(Product::sqlite_table(db))?;
```

`from_table` consumes the typed table and gives a `Vista` that knows its columns, references, id
column and the backend's capabilities. The schema is fixed from there; `Vista` has no
`add_column`.

With no Rust entity for the table, the same factory builds one from a YAML spec:

```rust
use vantage_vista::VistaFactory;

let products = factory.from_yaml(include_str!("products.yaml"))?;
```

A driver author wraps a shell directly:

```rust
use vantage_vista::{TableShell, Vista};

let vista = Vista::new("product", MyShell::new(conn));
```

`VistaFactory` and `TableShell` are imported from the crate root, not the prelude: only driver
code and spec loaders name them.

### Narrow

```rust
use vantage_vista::prelude::*;

let pastries = products
    .clone()
    .with_condition("category", FilterOp::Eq, "pastries")?
    .with_order("price", SortDirection::Descending)?;

let mut cheap = products.clone();
cheap.add_condition("price", FilterOp::Lt, 5)?;
cheap.set_search(Some("tart"))?;
```

`products` is untouched by both. Each clause is offered to the shell first and recorded only once
the shell accepts it, so `pastries.query()` is a `VistaQuery` holding exactly what was applied. A
clause the backend can't take is refused with an `Unsupported` error naming the `Capability` it
lacks. An unknown column is a plain error, so a typo is never mistaken for a missing feature.

### A query as a value

`VistaQuery` is plain data: `Condition`s (`column <op> value`), a search text, an order
(`(column, SortDirection)`) and the keys of any driver-native conditions. Build one and apply it:

```rust
let query = VistaQuery::new()
    .with_condition(Condition::new("category", FilterOp::Eq, "pastries"))
    .with_order("price", SortDirection::Ascending);

let (pastries, residual) = products.clone().with_query(&query)?;
```

`residual` holds the clauses the shell refused. A caller that needs every clause applied checks
it is empty; a cache or a client-side refine evaluates it over the rows itself, with
`residual.matches(&row, &metadata)` and `residual.sort_rows(&mut rows, &metadata)` (the evaluator
documented in `vantage_vista::eval`).
`query.key()` is a stable string for cache identity.

### Read and write

`Vista` implements `ValueSet`, `ReadableValueSet`, `WritableValueSet` and `InsertableValueSet`,
the same trait family `Table<T, E>` does:

```rust
let rows = pastries.list_values().await?;
let total = pastries.count().await?;
let page = pastries.fetch_window(40, 20).await?;   // page.rows, page.total
```

The leading window (`fetch_window(0, n)`) works on every shell. Any other offset needs
`Capability::Window`; a forward-only backend offers `fetch_cursor` under `Capability::Cursor`.
A write through a narrowed Vista needs `Capability::ConfineWrites`, so it can't land outside the
set.

### Capabilities

```rust
let caps = products.capabilities();
if caps.has(Capability::Insert) { /* show "+ New" */ }
if caps.has(Capability::Order) { /* render sortable headers */ }
if caps.has(Capability::Search) { /* render the quicksearch box */ }
if caps.has(Capability::Subscribe) { /* follow products.watch() */ }
```

Each `Capability` names exactly one method family, and a shell that advertises one must answer it
without refusing; the `contract` feature checks that. A read-only AWS API and a writable SQLite
table look the same from outside; the set tells the UI which controls to draw.

### Catalog and resolvers

`VistaResolver` is the one way to turn a name into a `Vista`: a model in a catalog, a relation's
target, a table a script names. Any `Fn(&str) -> Result<Vista>` is one. `Catalog` maps model names
to loaders, which may sit on different backends, and is itself a resolver:

```rust
use vantage_vista::Catalog;

let catalog = Catalog::new()
    .with("client", |name: &str| clients_vista(name))
    .with("order", |name: &str| orders_vista(name));

let orders = catalog.vista("order")?;
```

A name the resolver doesn't know is `not_found`; a loader that fails for another reason keeps its
own error kind, so a caller can tell a missing model from an unreachable one. Within one backend,
`vista.get_ref("orders", Some(&row))` traverses a declared reference; across backends, narrow the
catalog's target with `reference.narrow(catalog.vista(&reference.target)?, &row)`.

### Scripting with Rhai

The `rhai` feature adds the `rhai` module and its `DataVocab`, the one data vocabulary every
Vantage script uses:

```rhai
let due = table("order").where("status", "due");
due.ref("client").list()
```

`table(name)` returns a handle that narrows (`where`, `sort`, `search`, `limit`, `ref`) and then
reads (`list`, `get`, `first`, `count`, `ids`), writes (`insert`, `upsert`, `patch`, `delete`,
`import_from`) or opens a `record` draft. A host picks the tables behind `table(name)` and which
verbs exist. The full guide is [Scripting with Rhai](https://vantage-ui.com/book/rhai.html) in the
Vantage book.

### Why not a stateless shell?

A shell that held no query, with every read taking a `VistaQuery`, would make `Vista` a thin
wrapper and drivers simpler. It was considered and rejected: `Vista` would stop matching
`Table<T, E>`, which owns its conditions, and code written against the dataset traits would need a
query threaded through every call. The cost of the path taken is that a driver's shell is `Clone`
and carries its narrowing, and a clone must not share it.

## Prelude

`use vantage_vista::prelude::*;` brings in `Vista`, `VistaQuery`, `Condition`, `FilterOp`,
`SortDirection`, `Capability`, `VistaResolver`, `CborValue`, and the vantage-dataset traits that
`Vista` implements (`ValueSet`, `ReadableValueSet`, `WritableValueSet`, `InsertableValueSet`).

Everything else is reached from the crate root: `VistaFactory` and `TableShell` for driver
authors, `Catalog`, `Column`, `Reference`, `Page`, `VistaMetadata`. `VistaCapabilities` is what
`vista.capabilities()` returns.

## Features

- `mocks`: `vantage_vista::mocks::MockShell`, an in-memory `TableShell` for tests and examples.
  Enable it from `[dev-dependencies]`.
- `rhai`: the `rhai` module (`DataVocab`, the table handle, script-slot evaluators,
  `run_script`). Backends add their vendor expression vocabulary through
  `TableShell::register_rhai_extensions`.
- `contract`: `vantage_vista::contract`, the driver-facing checks every backend's test suite runs
  against its own fixture. Enable it from `[dev-dependencies]`.

A test Vista over `MockShell`:

```rust
use vantage_vista::mocks::MockShell;

let users = Vista::new("user", MockShell::new().with_record("u1", record));
```

## Integration

- vantage-dataset: the `ValueSet` traits `Vista` implements
- vantage-types: the `Record<V>` carrier
- vantage-core: `Result` / `VantageError`
- vantage-rhai: the script host behind the `rhai` feature
- vantage-diorama: the `Lens`/`Dio` cache and the sceneries that sit on a `Vista`
