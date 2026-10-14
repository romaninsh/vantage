# Vista — the Universal Data Handle

Chapters 1–3 built a typed data layer: `Table<SqliteDB, Product>`, conditions, relationships, CRUD.
That layer is great when you know the entity at compile time. But step 3's generic `crud()` helper
already showed the limitation — it could only exist because we erased the entity type with
`Serialize + DeserializeOwned` and went through JSON.

A CLI that lists "any table from any backend", a web admin that draws forms from a YAML schema, a UI
data grid that works with whatever you point it at — none of these know your `Product` struct. They
need a handle that carries its own schema, speaks a single value type, and works regardless of which
database sits underneath.

That handle is [`Vista`](vantage_vista::Vista).

```admonish question title="Why 'Vista'?"
Vantage is named for the view. Stand at a vantage point — the peak above your infrastructure — and
the landscape arranges itself below: every database, API, and file your organisation keeps,
observable from one place. The framework's names stay true to that scene, borrowing from landscape
and photography rather than reusing overloaded database words.

A **table** isn't one of ours — it's what you already have. Vantage describes your tables; it
doesn't reinvent them. A **vista** is the first thing the framework adds: in the landscape sense, a
wide view opening up from where you stand. And that is precisely what a `Vista` is here — one data
source seen in full, its schema, capabilities, and records composed into a single view, no matter
what produces it. Further along the trail wait a **Lens**, a **Dio**rama, and **Scenery** — each
another way of looking, never another thing to store.
```

```admonish example title="Goals for this chapter"
By the end of this page you'll be able to:

1. Wrap a typed `Table` into a Vista
2. Read schema metadata (columns, references, id column) from a Vista
3. Narrow a Vista with conditions, search, and ordering
4. Read a window of rows with `fetch_window`, or walk a cursor with `fetch_cursor`
5. Traverse relationships and cross-backend references
6. Understand capabilities — the explicit contract between Vista and its driver
```

---

## What Vista actually is

A `Vista` wraps a typed `Table<DB, E>` and erases both the backend and the entity. All data flows
through `Record<CborValue>` — an ordered map of string keys to CBOR values. All schema lives on the
Vista itself: columns (with types and flags), references, id column, and a set of capabilities.

Think of the progression:

```text
Table<SqliteDB, Product>   — typed entity, typed backend, compile-time safe
Vista                      — fully erased: schema-bearing, CborValue, no generics
```

Vista trades away compile-time knowledge for runtime flexibility: everything is a string key and a
CBOR value, and the schema travels with the handle rather than with your types. Wrapping a typed
`Table` — this chapter's path — is also not the only way to get one: every driver ships a
`VistaFactory` that can materialize a Vista from a declarative YAML spec, with Rhai scripts for the
expressions YAML can't state. That path has its own guide —
[Config-Driven Vistas](../config-driven-vistas.md) — and this chapter stays on the typed one.
Scripts read and write Vistas through their own short vocabulary (`table("product").where(…)`),
taught in [Scripting with Rhai](../rhai.md); the methods below are the Rust side of the same
operations.

```admonish info title="CBOR, not JSON"
Vista uses [`ciborium::Value`](https://docs.rs/ciborium) as its carrier type — a CBOR value. CBOR preserves type fidelity that JSON loses (integer vs float, binary blobs, precise decimals). You'll see `CborValue` in every Vista method signature.

If you need JSON (for an HTTP response, for example), convert at the boundary — `Record<CborValue>` → `Record<serde_json::Value>` is a one-liner. But inside the Vista layer, CBOR is the standard.
```

---

## Wrapping a typed Table

Each backend ships a factory that turns a typed table into a Vista. For SQLite, the database hands
you one:

```rust,no_run
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# use vantage_vista::prelude::*;
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Product { name: String, price: i64 }
# impl Product {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("category_id")
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
let table = Product::table(db.clone());
let vista: Vista = db.vista_factory().from_table(table)?;
# Ok(())
# }
```

That's it. The factory harvests columns, id field, title fields, and references from the typed table
definition you already built in chapter 2. No extra mapping code.

For MongoDB the factory is a `MongoVistaFactory`, for AWS an `AwsVistaFactory` — same shape,
different import. The resulting `Vista` is the same type regardless of which factory produced it.

This chapter's snippets read `products.db`, the file chapters 1–3 built, so they compile but are not
run by the book's tests.

---

## Reading schema

Once you have a Vista, everything is runtime introspection:

```rust,no_run
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# use vantage_vista::prelude::*;
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Product { name: String, price: i64 }
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Category { name: String }
# impl Product {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("category_id")
#             .with_one("category", "category_id", Category::table)
#     }
# }
# impl Category {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Category> {
#         Table::new("category", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_many("products", "category_id", Product::table)
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
# let vista = db.vista_factory().from_table(Product::table(db.clone()))?;
// Columns — name, original type, flags
for (name, column) in vista.columns() {
    let ty = column.original_type.as_deref().unwrap_or("?");
    println!("{name}: {ty}");
}
// price: i64
// …

// ID column
if let Some(id_column) = vista.id_column() {
    println!("id column: {id_column}");
}

// Title columns — the human-readable ones
for title in vista.title_columns() {
    println!("title: {title}");
}

// References — relationships declared on the typed table
for (name, reference) in vista.references() {
    println!("ref: {name} ({})", reference.kind.as_str());
}
// ref: category (has_one)
# Ok(())
# }
```

[`Column`](vantage_vista::Column) carries an `original_type` (the Rust type name the typed column
was declared with, or `None` when nothing declares one) and a set of
[`ColumnFlag`](vantage_vista::ColumnFlag)s. The flags the data layer acts on:

| Flag           | Meaning                                                  |
| -------------- | -------------------------------------------------------- |
| `Id`           | This column is the primary key                           |
| `Title`        | Human-readable label column                              |
| `Searchable`   | Included in quicksearch                                  |
| `Orderable`    | Can be sorted server-side                                |
| `Hidden`       | Don't show in default views                              |
| `Mandatory`    | Required on insert                                       |
| `RequestParam` | Sent to the backend as a request parameter (a URL slot)  |
| `Calculated`   | Computed, not stored; read-only, and writes drop it      |
| `Auto`         | The backend makes the value (auto-increment, record ids) |

Any other name is kept as `ColumnFlag::Other` for the layer that reads it — a UI's presentation
flag, for instance. Vista itself never interprets those.

---

## Narrowing

Vista doesn't carry its own condition type. Instead, it delegates to a
[`TableShell`](vantage_vista::TableShell) — the per-driver executor every Vista wraps, and the
piece each factory actually builds — which translates the value into whatever the backend speaks:

```rust,no_run
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# use vantage_vista::prelude::*;
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Product { name: String, price: i64 }
# impl Product {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("category_id")
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
# let vista = db.vista_factory().from_table(Product::table(db.clone()))?;
let cakes = vista.clone().with_condition("category_id", FilterOp::Eq, 1)?;

let rows = cakes.list_values().await?;
// Only products with category_id == 1; `vista` still holds every product
# Ok(())
# }
```

The value converts into a `CborValue`. The driver's shell translates the condition into a native
one — an `Expression` for SQL, a `bson::Document` for MongoDB, an `AwsCondition` for AWS — and
pushes it onto the wrapped table. [`FilterOp`](vantage_vista::FilterOp) has the other comparisons
too (`Gt`, `In`, …); every shell takes `Eq`, and the rest need the `FilterOps` capability.

### Why `with_*` consumes

A `Vista` is a value, the same as the `Table` it wraps. It owns its query state, so
`vista.clone().with_condition(..)` hands back a new, smaller set and leaves `vista` exactly as it
was. That's the property generic code leans on: a function that receives a Vista can narrow it for
its own purposes without changing what the caller sees, and a cached "all products" Vista can be the
starting point for every request that needs a subset.

The `with_*` methods take `self` and return `Result<Self>`, so they chain:

```rust,no_run
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# use vantage_vista::prelude::*;
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Product { name: String, price: i64 }
# impl Product {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("category_id")
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
# let vista = db.vista_factory().from_table(Product::table(db.clone()))?;
let v = vista
    .clone()
    .with_condition("category_id", FilterOp::Eq, 1)?
    .with_order("price", SortDirection::Descending)?;
# Ok(())
# }
```

When you hold a Vista of your own and want to change it in place — a UI handle whose search box
the user keeps editing — the `&mut self` methods do the same work: `add_condition`, `set_search`,
`set_order`. They exist for the owner of the value, not for narrowing someone else's.

| Operation                                        | Semantics                                                    |
| ------------------------------------------------ | ------------------------------------------------------------ |
| `with_condition`, `with_id` / `add_condition`    | **Accrete** — each call narrows further; none can be removed |
| `with_search`, `with_order` / `set_search`, `set_order` | **Replace** — a later call swaps the earlier one out; `None` clears it |
| Widening                                         | **Clone first** — narrow the clone, keep the original        |

A narrowed Vista is a *different, smaller set* — not a view you toggle filters on. Each clause the
shell accepted is recorded on [`query()`](vantage_vista::Vista::query), so the layers above (the
cache in the next chapter, for one) can tell two narrowed Vistas apart.

### Narrowing by id

A common pattern — "I have an id and want the row":

```rust,no_run
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# use vantage_vista::prelude::*;
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Product { name: String, price: i64 }
# impl Product {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("category_id")
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
# let vista = db.vista_factory().from_table(Product::table(db.clone()))?;
let row = vista.clone().with_id(7)?.get_some_value().await?;
// Some(("7", record)) when the row exists
# Ok(())
# }
```

[`with_id`](vantage_vista::Vista::with_id) reads the id column name from the schema and adds the
equality condition for you.

---

## Search and ordering

```rust,no_run
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# use vantage_vista::prelude::*;
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Product { name: String, price: i64 }
# impl Product {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("category_id")
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
# let vista = db.vista_factory().from_table(Product::table(db.clone()))?;
// Quicksearch — fans across columns flagged Searchable — then sort by column
let v = vista
    .clone()
    .with_search("tart")?
    .with_order("price", SortDirection::Descending)?;

let rows = v.list_values().await?;
# Ok(())
# }
```

Both have **replace semantics** — a second search drops the first. Both return an error if the
driver doesn't support them, and ordering also refuses a column that isn't flagged `Orderable`.
Check capabilities first, or try and handle the error.

```admonish warning title="Not every driver supports these"
A CSV file can't sort or search server-side — it loads everything into memory. DynamoDB can only order by its declared sort key. Each driver advertises its capabilities accordingly. Calling a method the driver doesn't advertise returns an `Unsupported` error naming the capability. This is by design — it's better to fail clearly than to silently return unfiltered results.
```

Generic code branches on the capabilities instead, here on a Vista it owns:

```rust,no_run
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# use vantage_vista::prelude::*;
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Product { name: String, price: i64 }
# impl Product {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
# let vista = db.vista_factory().from_table(Product::table(db.clone()))?;
let mut v = vista.clone();
if v.capabilities().has(Capability::Search) {
    v.set_search(Some("query"))?;
}
if v.capabilities().has(Capability::Order) {
    v.set_order(Some(("name", SortDirection::Ascending)))?;
}
# Ok(())
# }
```

---

## Reading windows

Vista has two ways to read part of a set. Not every driver supports both — capabilities tell you
which to use.

### A window by offset: `fetch_window`

Random access — any `[offset, offset + limit)` slice of the set, in its order:

```rust,no_run
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# use vantage_vista::prelude::*;
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Product { name: String, price: i64 }
# impl Product {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
# let vista = db.vista_factory().from_table(Product::table(db.clone()))?;
let first = vista.fetch_window(0, 25).await?;    // rows 0..25
let second = vista.fetch_window(25, 25).await?;  // rows 25..50

println!("{} rows, total {:?}", first.rows.len(), first.total);
# Ok(())
# }
```

A [`Page`](vantage_vista::Page) holds the rows and, when the fetch learned it, the set's total —
`None` means "didn't say", never zero. The leading window (`offset == 0`) works on every driver
that lists; any other offset needs the `Window` capability. SQL databases and MongoDB have it,
anything with `LIMIT … OFFSET`.

### A cursor: `fetch_cursor`

Forward only — each call returns up to `limit` rows and a token for the next call:

```rust,no_run
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# use vantage_vista::prelude::*;
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Product { name: String, price: i64 }
# impl Product {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
# let vista = db.vista_factory().from_table(Product::table(db.clone()))?;
if vista.capabilities().has(Capability::Cursor) {
    let (page, token) = vista.fetch_cursor(None, 100).await?;   // first page
    let (page, token) = vista.fetch_cursor(token, 100).await?;  // second page
    // token is None when the set is exhausted
}
# Ok(())
# }
```

The token is **opaque** — its shape is driver-private (a DynamoDB `LastEvaluatedKey`, a REST
`nextToken`, the last key S3 returned). Just round-trip it. Requires the `Cursor` capability, which
SQLite doesn't advertise and the S3 listing in the next chapter does.

```admonish info title="When neither is available"
Without `Window` and `Cursor`, the leading window and `list_values()` are what the driver offers. That's the CSV case — fine for small datasets.
```

---

## Traversing references

Same-persistence references (the `with_many` / `with_one` from chapter 2) come through
automatically. Given a parent row, `get_ref` resolves the relationship:

```rust,no_run
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# use vantage_vista::prelude::*;
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Product { name: String, price: i64 }
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Category { name: String }
# impl Product {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("category_id")
#             .with_one("category", "category_id", Category::table)
#     }
# }
# impl Category {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Category> {
#         Table::new("category", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_many("products", "category_id", Product::table)
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
let categories = db.vista_factory().from_table(Category::table(db.clone()))?;

// Given a category row, traverse to its products
if let Some((_, category)) = categories.get_some_value().await? {
    let products = categories.get_ref("products", Some(&category))?;
    for (id, row) in products.list_values().await? {
        println!("  {id} — {:?}", row.get("name"));
    }
}
# Ok(())
# }
```

`get_ref` takes the parent row as a `Record<CborValue>`. It reads the join field (e.g.
`category_id`) out of that row to build the condition on the target, so a traversal starts from a
row: fetch the parent with `get_some_value()` or `list_values()`, then traverse from each row. With
`None` in place of the row, you get the bare target — the set a new related row goes into.

`get_ref` resolves two kinds of relation:

1. **Contained relations** — records embedded in a column of the parent row (an order's `lines`
   array) come back as an editable sub-Vista.
2. **Foreign-key references** — the shell forwards to the `with_one` / `with_many` relationships
   declared on the typed table, and re-wraps the narrowed target as a fresh Vista.

Either way the result is another `Vista`, so traversal chains without ever leaving the universal
surface. Traversal needs the `Traverse` capability.

### Cross-backend references

What if your categories live in PostgreSQL and your products in MongoDB? That is deliberately
*not* a Vista's job — a Vista honestly describes one backend, and there is no engine that could
execute a join across two. Cross-persistence traversal lives one layer up, in a
[`Catalog`](vantage_vista::Catalog): register a loader per model name, and
`reference.narrow(catalog.vista(&reference.target)?, &row)` builds the target in *its* backend and
narrows it by values read from the parent row. The consumer still just
receives a Vista — see [Config-Driven Vistas](../config-driven-vistas.md) for the catalog in
action.

---

## Capabilities — the explicit contract

[`VistaCapabilities`](vantage_vista::VistaCapabilities) is a set of
[`Capability`](vantage_vista::Capability) values. The driver puts in exactly what it implements:

```rust,no_run
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# use vantage_vista::prelude::*;
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# struct Product { name: String, price: i64 }
# impl Product {
#     fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
# let vista = db.vista_factory().from_table(Product::table(db.clone()))?;
let caps = vista.capabilities();
for capability in caps.iter() {
    println!("{capability}");
}
// order, search, filter_ops, window, traverse, insert, update, delete, …

if caps.has(Capability::Insert) {
    println!("writable");
}
# Ok(())
# }
```

Each capability names one method family: `Insert`, `Update`, `Delete`, `Import`, `Subscribe`,
`Order`, `Search`, `FilterOps`, `NativeConditions`, `Window`, `Cursor`, `Traverse` and
`ConfineWrites` (writes through a narrowed Vista stay inside its set). Listing, getting a row,
counting and equality conditions have no capability: every driver answers them.

A CSV file advertises `Window` and `Traverse` — it's read-only. SQLite advertises reads, writes,
`Window` and `Traverse`, but not `Cursor`. An S3 listing advertises `Cursor` and `Traverse`: no
random access, no search, no writes.

```admonish warning title="Calling unsupported methods is an error"
The capabilities aren't suggestions — they're a contract. If `Search` isn't in the set, `set_search()` returns an `Unsupported` error, which `VantageError::is_unsupported()` recognises. A contract test calls every advertised capability on every shell in the workspace, so a driver can't advertise a method it doesn't answer.

UI adapters branch on these to decide which controls to show. A data grid checks `Window` to decide between a scrollbar (random access) and a "load more" button (`Cursor`).
```

---

## Putting it together

Here's a small function that takes any Vista and prints a summary — works with any backend:

```rust
# use vantage_core::Result as VantageResult;
# use vantage_vista::prelude::*;
async fn print_vista(vista: &Vista) -> VantageResult<()> {
    let columns: Vec<&String> = vista.columns().keys().collect();

    // Header
    print!("  {:>12} ", vista.id_column().unwrap_or("id"));
    for col in &columns {
        print!("{col:>16} ");
    }
    println!();

    // Count — every driver answers it
    println!("  ({} rows)", vista.count().await?);

    // Rows
    let rows = vista.list_values().await?;
    for (id, record) in &rows {
        print!("  {id:>12} ");
        for col in &columns {
            let val = record
                .get(col.as_str())
                .map(|v| format!("{v:?}"))
                .unwrap_or_default();
            print!("{val:>16} ");
        }
        println!();
    }
    Ok(())
}
```

No generics. No entity type. No backend knowledge. `print_vista` works with a SQLite Vista, a
MongoDB Vista, an AWS Vista — anything the framework can produce.

---

## What we covered

| Concept                                                 | What it does                                            |
| ------------------------------------------------------- | ------------------------------------------------------- |
| [`Vista`](vantage_vista::Vista)                         | Universal schema-bearing data handle, wraps any `Table` |
| [`TableShell`](vantage_vista::TableShell)               | Per-driver executor that Vista delegates to             |
| [`VistaCapabilities`](vantage_vista::VistaCapabilities) | Explicit contract of what the driver supports           |
| [`Column`](vantage_vista::Column)                       | Column metadata with name, type, and flags              |
| `columns()` / `id_column()` / `references()`            | Read the schema at runtime                              |
| `clone().with_condition(..)`                            | Narrow a copy; the original is untouched                |
| `with_id`                                               | Convenience: narrow by primary key                      |
| `with_search` / `with_order`                            | Quicksearch and sorting (replace semantics)             |
| `add_condition` / `set_search` / `set_order`            | The same, in place, on a Vista you own                  |
| `fetch_window` / `fetch_cursor`                         | Offset windows and cursor pages                         |
| `get_ref`                                               | Traverse a relationship from a parent row               |
| [`Catalog`](vantage_vista::Catalog)                     | Cross-persistence traversal, one layer above Vista      |

```admonish tip title="What's next"
Vista gives you a universal read/write handle. But every call still hits the database — there's no caching, no reactivity, no way to push live updates to a UI.

The next chapter introduces **Dio** and **Lens** — the caching and event layer that sits between a Vista (your master data) and a Scenery (the reactive view a UI consumes).
```
