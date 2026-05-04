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
use vantage_sql::sqlite::SqliteVistaFactory;

let factory = SqliteVistaFactory::new(db.clone());
let products: Vista = factory.from_table(Product::sqlite_table(db));
```

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
if caps.can_insert    { /* show "+ New" */ }
if caps.can_subscribe { /* wire live updates */ }

match caps.paginate_kind {
    PaginateKind::None   => /* fetch everything */,
    PaginateKind::Offset => /* limit/offset */,
    PaginateKind::Cursor => /* opaque cursor */,
}
```

A read-only AWS API and a writable SQLite table look the same shape from outside; the capability
flags tell the UI which buttons to draw.

## Narrow with conditions

Exact-equality only at this stage:

```rust,ignore
products.add_condition_eq("category", CborValue::Text("pastries".into()))?;

let pastries = products.list_values().await?;   // honours the filter
let count    = products.get_count().await?;     // also honours it
```

A conditioned Vista is a narrowed view of the data set, not a query result — same mental model as
`Table::with_condition`. Richer conditions arrive later without breaking this surface.

## CRUD over CBOR

Vista's wire type is `ciborium::Value`, regardless of what the backend stores natively. CSV strings,
MongoDB BSON, SurrealDB CBOR records, AWS JSON-1.1 blobs — drivers translate to and from CBOR at the
boundary. That's what lets one `render` work over all of them. Ids are `String` for the same reason:
Mongo `ObjectId`, Surreal `Thing`, composite keys all stringify in the driver.

Vista implements `ValueSet`, `ReadableValueSet`, `WritableValueSet`, and `InsertableValueSet` — the
same trait family `Table<T, E>` does, so code written against those traits works against a `Vista`
unchanged.

## Status

Incubating. Ships `Vista`, `TableShell`, `VistaMetadata`, `VistaCapabilities`, `Column`,
`Reference`, plus `MockShell` for tests. Driver factories (SQLite, Mongo, AWS, …) and the YAML
loader land in later crates.

## Integration

- vantage-dataset — `ValueSet` traits Vista implements
- vantage-types — `Record<V>` carrier
- vantage-core — `Result` / `VantageError`
