# Vantage Vista

`Vista` takes a typed table — the kind you define in your model layer like
`bakery_model3::Product::sqlite_table()` or `vantage_aws::models::ecs::clusters_table()` — and wraps
it behind a uniform handle that generic code can drive.

Vantage's `Table<T, E>` is typed end-to-end, which is what business logic wants. Anything generic —
CLI, desktop UI, config-driven dashboard — can't consume that without giving up the erasure it
needs. Vista is the bridge.

## Build a Vista

Define the table once, in the model layer. Then wrap it with the matching driver factory:

```rust,ignore
use bakery_model3::Product;
use vantage_sql::sqlite::vista::SqliteVistaFactory;

let factory = SqliteVistaFactory::new(db.clone());
let products: Vista = factory.from_table(Product::sqlite_table(db))?;
```

Each driver also hands out its factory directly, so `db.vista_factory()` gives the same
`SqliteVistaFactory`. `from_table` returns `Result<Vista>`.

`from_table` consumes the typed table and produces a `Vista` that knows its columns, references, id
field, and the backend's capabilities. The schema is locked from there — `Vista` has no
`add_column`. Conditions can still be added; that's narrowing the data set, not the schema.

## Or skip the model crate

No Rust entity for the table? Hand the factory a YAML schema instead:

```rust,ignore
let products: Vista = factory.from_yaml(include_str!("products.yaml"))?;
```

Same `Vista`. Useful for CLIs that talk to arbitrary databases at runtime, or admin tools that load
schemas from configuration.

## Drive it

Anything taking `&Vista` works against any backend:

```rust,ignore
use vantage_dataset::ReadableValueSet;

async fn render(vista: &Vista) -> VantageResult<()> {
    for col in vista.get_column_names() {
        if vista.get_column(col).unwrap().is_hidden() { continue; }
        print!("{}\t", col);
    }
    println!();

    for (_id, row) in vista.list_values().await? {
        for col in vista.get_column_names() {
            if vista.get_column(col).unwrap().is_hidden() { continue; }
            print!("{:?}\t", row.get(col));
        }
        println!();
    }
    Ok(())
}

render(&products).await?;
```

Swap SQLite for Mongo, AWS, or CSV — `render` doesn't change.

## Obey what the backend can do

Drivers advertise capabilities so generic UIs render the right controls instead of guessing and
erroring at runtime:

```rust,ignore
let caps = vista.capabilities();
if caps.can_insert       { /* show "+ New" */ }
if caps.can_subscribe    { /* wire live updates */ }
if caps.can_order        { /* render column sort headers */ }
if caps.can_search       { /* render the quicksearch box */ }
if caps.can_set_page_size { /* expose page-size picker */ }
if caps.can_fetch_page   { /* show random-access page selector */ }
else if caps.can_fetch_next { /* show forward-only "Next" button */ }
else                      { /* no pagination — fetch everything */ }
```

A read-only AWS API and a writable SQLite table look the same shape from outside; the capability
flags tell the UI which buttons to draw.

## Narrow with conditions

Equality works on every backend:

```rust,ignore
products.add_condition_eq("category", CborValue::Text("pastries".into()))?;

let pastries = products.list_values().await?;   // honours the filter
let count    = products.get_count().await?;     // also honours it
```

A conditioned Vista is a narrowed view of the data set, not a query result — same mental model as
`Table::with_condition`.

Other operators go through `add_condition(field, FilterOp, value)` (`Ne`, `Gt`, `Gte`, `Lt`, `Lte`,
`InSet`, `NotInSet`, ...). They need the driver to set `can_filter_operators`; otherwise the call
returns an `Unimplemented` error and the caller filters locally:

```rust,ignore
if products.capabilities().can_filter_operators {
    products.add_condition("price", FilterOp::Gte, CborValue::Integer(100.into()))?;
}
```

`with_id`, `add_search` and `add_order(column, SortDirection)` narrow and sort the same way.

## CRUD over CBOR

Vista's wire type is `ciborium::Value`, regardless of what the backend stores natively. CSV strings,
MongoDB BSON, SurrealDB CBOR records, AWS JSON-1.1 blobs — drivers translate to and from CBOR at the
boundary. That's what lets one `render` work over all of them. Ids are `String` for the same reason:
Mongo `ObjectId`, Surreal `Thing`, composite keys all stringify in the driver.

Vista implements `ValueSet`, `ReadableValueSet`, `WritableValueSet`, and `InsertableValueSet` — the
same trait family `Table<T, E>` does, so code written against those traits works against a `Vista`
unchanged.

## Scripting with Rhai

The `rhai` feature adds `DataVocab`, the one data vocabulary every Vantage script uses:

```rhai
let due = table("order").where("status", "due");
due.ref("client").list()
```

`table(name)` returns a handle that narrows (`where`, `sort`, `search`, `limit`, `ref`) and then
reads (`list`, `get`, `first`, `count`, `ids`), writes (`insert`, `upsert`, `patch`, `delete`,
`import_from`) or opens a `record` draft. A host picks the tables behind `table(name)` and which
verbs exist. The full guide is
[Scripting with Rhai](https://vantage-ui.com/book/rhai.html) in the Vantage book.

## Status

Ships `Vista`, `TableShell`, `VistaFactory` (with a default `from_yaml`), `VistaMetadata`,
`VistaCapabilities`, `Column`, `Reference` and `ContainedSpec`, plus `MockShell`, `EmptyShell` and
`ForwardShell`. On top of reads and writes, a Vista does paging (`set_page_size`, `fetch_page`,
`fetch_next`, `fetch_window`), aggregates (`aggregate(&AggregateSpec)` returns
`Result<Option<Vista>>`, `None` when the driver can't answer it), reference traversal
(`get_ref(relation, row)`, `get_ref_target`) and live changes (`can_watch`,
`watch()` returning a `VistaChangeStream`). Drivers that can't do one of these say so through
`capabilities()` or return `None` / an error. Driver factories
(`SqliteVistaFactory`, `MongoVistaFactory`, `CsvVistaFactory`, `AwsVistaFactory`, ...) live in the
driver crates.

## Integration

- vantage-dataset — `ValueSet` traits Vista implements
- vantage-types — `Record<V>` carrier
- vantage-core — `Result` / `VantageError`
- vantage-rhai — script host behind the optional `rhai` feature
- vantage-expressions and vantage-table depend on this crate (`AnyExpression` and `ExpressionLike`
  live here), so drivers can build a Vista straight from a typed `Table`
- vantage-vista-factory — `VistaCatalog`, a name-to-Vista registry that also follows references
  across datasources
