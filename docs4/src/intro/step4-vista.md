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

```admonish example title="Goals for this chapter"
By the end of this page you'll be able to:

1. Wrap a typed `Table` into a Vista
2. Read schema metadata (columns, references, id column) from a Vista
3. Add conditions, search, and ordering through the Vista API
4. Fetch paginated results with `fetch_page` and `fetch_next`
5. Traverse relationships and cross-backend references
6. Understand capabilities — the honesty contract between Vista and its driver
```

---

## What Vista actually is

A `Vista` wraps a typed `Table<DB, E>` and erases both the backend and the entity. All data flows
through `Record<CborValue>` — an ordered map of string keys to CBOR values. All schema lives on the
Vista itself: columns (with types and flags), references, id column, and a set of capability flags.

Think of the progression:

```text
Table<SqliteDB, Product>   — typed entity, typed backend, compile-time safe
Vista                      — fully erased: schema-bearing, CborValue, no generics
```

Vista trades away compile-time knowledge for runtime flexibility. Everything is a string key and a
CBOR value — but it carries enough metadata to build a data grid, a form, a CLI, or a REST endpoint
without knowing anything about the underlying database.

```admonish info title="CBOR, not JSON"
Vista uses [`ciborium::Value`](https://docs.rs/ciborium) as its carrier type — a CBOR value. CBOR preserves type fidelity that JSON loses (integer vs float, binary blobs, precise decimals). You'll see `CborValue` in every Vista method signature.

If you need JSON (for an HTTP response, for example), convert at the boundary — `Record<CborValue>` → `Record<serde_json::Value>` is a one-liner. But inside the Vista layer, CBOR is the standard.
```

---

## Wrapping a typed Table

Each backend ships a factory that turns a typed table into a Vista. For SQLite:

```rust
use vantage_sql::prelude::*;
use vantage_vista::Vista;

let table = Product::table(db.clone());
let vista = SqliteVistaFactory::new(db).from_table(table)?;
```

That's it. The factory harvests columns, id field, title fields, and references from the typed table
definition you already built in chapter 2. No extra mapping code.

For MongoDB it would be `MongoVistaFactory`, for AWS it would be `AwsVistaFactory` — same shape,
different import. The resulting `Vista` is identical regardless of which factory produced it.

---

## Reading schema

Once you have a Vista, everything is runtime introspection:

```rust
// Columns — name, original type, flags
for name in vista.get_column_names() {
    let col = vista.get_column(name).unwrap();
    println!("{}: {}", col.name, col.original_type);
}
// name: String
// price: i64
// is_deleted: bool

// ID column
if let Some(id_col) = vista.get_id_column() {
    println!("id column: {}", id_col);
}

// Title columns — the human-readable ones
for title in vista.get_title_columns() {
    println!("title: {}", title);
}

// References — relationships declared on the typed table
for (name, kind) in vista.list_references() {
    println!("ref: {} ({:?})", name, kind);
}
// ref: products (HasMany)
```

[`Column`](vantage_vista::Column) carries an `original_type` string (preserved from the typed column
definition) and a set of flags. The standard flag vocabulary:

| Flag           | Meaning                        |
| -------------- | ------------------------------ |
| `"id"`         | This column is the primary key |
| `"title"`      | Human-readable label column    |
| `"searchable"` | Included in quicksearch        |
| `"orderable"`  | Can be sorted server-side      |
| `"hidden"`     | Don't show in default views    |
| `"mandatory"`  | Required on insert             |

Flags are open — drivers and consumers can add their own.

---

## Adding conditions

Vista doesn't carry its own condition type. Instead, it delegates to the wrapped driver, which
translates the value into whatever the backend speaks:

```rust
let mut v = vista.clone();
v.add_condition_eq("category_id", 1.into())?;

let rows = v.list_values().await?;
// Only products with category_id == 1
```

`.into()` converts the `i64` into a `CborValue`. The driver's shell translates that into a native
condition — `Expression` for SQL, `bson::Document` for MongoDB, `AwsCondition::Eq` for AWS — and
pushes it onto the wrapped table.

```admonish info title="Conditions mutate the shell"
Unlike `Table`'s `.with_condition()` (which consumes and returns a new table), Vista's `add_condition_eq` mutates in place. Vista is a runtime handle — there's no builder pattern to preserve. Clone before narrowing if you need the unfiltered version later.
```

### Narrowing by id

A common pattern — "I have an id and want the row":

```rust
let mut v = vista.clone();
v.with_id("7")?;
let row = v.get_some_value().await?;
```

[`with_id`](vantage_vista::Vista::with_id) reads the id column name from the schema and calls
`add_condition_eq` for you. Returns `&mut Self` so you can chain.

---

## Search and ordering

```rust
let mut v = vista.clone();

// Quicksearch — fans across columns flagged SEARCHABLE
v.add_search("tart")?;

// Sort by column
v.add_order("price", SortDirection::Descending)?;

let rows = v.list_values().await?;
```

Both are **replace semantics** — calling again drops the previous filter/order. Both return an error
if the driver doesn't support them. Check capabilities first (or just try and handle the error).

```admonish warning title="Not every driver supports these"
A CSV file can't sort or search server-side — it loads everything into memory. DynamoDB can only order by its declared sort key. The driver sets capability flags honestly:

~~~rust
let caps = vista.capabilities();
if caps.can_search {
    v.add_search("query")?;
}
if caps.can_order {
    v.add_order("name", SortDirection::Ascending)?;
}
~~~

Calling a method the driver doesn't advertise returns an `Unsupported` error. This is by design — it's better to fail clearly than to silently return unfiltered results.
```

---

## Pagination

Vista exposes two pagination primitives. Not every driver supports both — capabilities tell you
which to use.

### Offset pagination: `fetch_page`

Random-access — jump to any page by number:

```rust
let mut v = vista.clone();
v.set_page_size(25)?;

let page1 = v.fetch_page(1).await?;   // first 25 rows
let page2 = v.fetch_page(2).await?;   // next 25 rows
```

Works for SQL databases, MongoDB, anything with `LIMIT … OFFSET`. Requires `can_fetch_page`.

### Cursor pagination: `fetch_next`

Forward-only — each call returns a token for the next:

```rust
let v = vista.clone();

let (rows, token) = v.fetch_next(None).await?;        // first page
let (rows, token) = v.fetch_next(token).await?;      // second page
// token is None when exhausted
```

The token is **opaque** — its shape is driver-private (a DynamoDB `LastEvaluatedKey`, a REST
`nextToken`, an offset counter). Just round-trip it. Requires `can_fetch_next`.

```admonish info title="When neither is available"
If both `can_fetch_page` and `can_fetch_next` are `false`, the driver has no native pagination. Fall through to `list_values()` which returns everything. This is the CSV case — fine for small datasets.
```

---

## Traversing references

Same-persistence references (the `with_many` / `with_one` from chapter 2) come through
automatically. Given a parent row, `get_ref` resolves the relationship:

```rust
// Given a category row, traverse to its products
let products = vista.get_ref("products", &category_row)?;
for (id, row) in products.list_values().await? {
    println!("  {} — {:?}", id, row["name"]);
}
```

`get_ref` needs a `Record<CborValue>` — the parent row. It reads the join field (e.g. `category_id`)
out of that row to build the eq-condition on the target. You can't traverse from a Vista alone; you
need a row first. Typical flow: fetch the parent with `get_some_value()` or `list_values()`, then
traverse from each row.

`get_ref` routes in order:

1. **Foreign resolver** — checks for a [`with_foreign`](#cross-backend-references-with_foreign)
   closure registered under that name.
2. **Driver shell fallback** — delegates to the shell's `get_ref`, which resolves the `with_one` /
   `with_many` relationship declared on the typed table.

So the same `get_ref("products", &row)` call works whether the relationship lives in the same
database or crosses a backend boundary — Vista handles the routing transparently.

### Cross-backend references: `with_foreign`

Same-persistence refs work because the driver's shell can translate them natively. But what if your
categories live in PostgreSQL and your products in MongoDB? `with_foreign` registers a
cross-persistence resolver:

```rust
categories.with_foreign(
    "products",
    ReferenceKind::HasMany,
    |row| {
        let mut p = mongo_factory.from_table(Product::table(mongo_db.clone()))?;
        p.add_condition_eq("category_id", row["id"].clone())?;
        Ok(p)
    },
);
// get_ref("products", &row) now crosses from Postgres into MongoDB
```

The closure is **stored, not called** at registration. It fires once — lazily — when
`get_ref("products", &row)` is invoked. This avoids recursion when two Vistas reference each other.

The closure receives the parent row and returns a Vista from **any** backend. The consumer calling
`get_ref` doesn't know or care that the join crossed a database boundary.

```admonish tip title="Lazy resolution"
Because `with_foreign` closures are lazy, you can register mutual references between two Vistas (A → B and B → A) without worrying about construction order. The closures capture factories by clone, not by reference to a Vista that doesn't exist yet.
```

---

## Capabilities — the honesty contract

[`VistaCapabilities`](vantage_vista::VistaCapabilities) is a struct of booleans. The driver sets
each flag to reflect what it actually implements:

```rust
let caps = vista.capabilities();
println!("can_count: {}", caps.can_count);
println!("can_insert: {}", caps.can_insert);
println!("can_update: {}", caps.can_update);
println!("can_delete: {}", caps.can_delete);
println!("can_order: {}", caps.can_order);
println!("can_search: {}", caps.can_search);
println!("can_fetch_page: {}", caps.can_fetch_page);
println!("can_fetch_next: {}", caps.can_fetch_next);
```

A CSV file sets `can_count` and nothing else — it's read-only. A SQL database sets everything. AWS
DynamoDB sets `can_count` and `can_fetch_next` (cursor-only) but not `can_fetch_page` (no random
access).

```admonish warning title="Calling unsupported methods is an error"
The capability flags aren't suggestions — they're a contract. If `can_search` is `false`, calling `add_search()` returns an `Unsupported` error. If a flag is `true` but the driver forgot to implement the method, you get an `Unimplemented` error instead. Both are `VantageError` variants you can match on.

UI adapters branch on these flags to decide which controls to show. A data grid checks `can_fetch_page` to decide between a scrollbar (random access) and a "load more" button (cursor-based).
```

---

## Putting it together

Here's a small function that takes any Vista and prints a summary — works with any backend:

```rust
use vantage_vista::Vista;

async fn print_vista(vista: &Vista) -> VantageResult<()> {
    let columns = vista.get_column_names();
    let caps = vista.capabilities();

    // Header
    print!("  {:>12} ", vista.get_id_column().unwrap_or("id"));
    for col in &columns {
        print!("{:>16} ", col);
    }
    println!();

    // Count
    if caps.can_count {
        println!("  ({} rows)", vista.get_count().await?);
    }

    // Rows
    let rows = vista.list_values().await?;
    for (id, record) in &rows {
        print!("  {:>12} ", id);
        for col in &columns {
            let val = record
                .get(col)
                .map(|v| format!("{:?}", v))
                .unwrap_or_default();
            print!("{:>16} ", val);
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
| [`VistaCapabilities`](vantage_vista::VistaCapabilities) | Honest contract of what the driver supports             |
| [`Column`](vantage_vista::Column)                       | Column metadata with name, type, and flags              |
| `add_condition_eq`                                      | Narrow results to field == value                        |
| `with_id`                                               | Convenience: narrow by primary key                      |
| `add_search` / `add_order`                              | Quicksearch and sorting (replace semantics)             |
| `fetch_page` / `fetch_next`                             | Offset and cursor pagination                            |
| `get_ref`                                               | Traverse a same-persistence relationship                |
| `with_foreign`                                          | Register a cross-persistence relationship resolver      |

```admonish tip title="What's next"
Vista gives you a universal read/write handle. But every call still hits the database — there's no caching, no reactivity, no way to push live updates to a UI.

The next chapter introduces **Dio** and **Lens** — the caching and event layer that sits between a Vista (your master data) and a Scenery (the reactive view a UI consumes).
```
