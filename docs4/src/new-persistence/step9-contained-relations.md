# Step 9: Contained Relations

Some data doesn't live in its own table. A product carries an `inventory` object; an order carries
a `lines` array. The records are real: they have fields, and you want to list them, add to them and
edit one. But they're embedded in a column of the parent row, not stored in a table of their own.

`with_one` / `with_many` (Step 6) can't model this: they resolve to *another* table through a
foreign key. A contained relation resolves to a **sub-Vista backed by one column of the same row**.
Reads project that column into records; writes patch the column back in place. To the consumer it
looks like any other relation: `get_ref("lines", Some(&row))`, then `list_values` / `insert_value` /
`patch_value` / `delete`.

This step is optional. Skip it unless your backend stores embedded objects or arrays that users
should edit as records.

### What the framework gives you

Almost all of it is backend-agnostic and already done.

**Declaration** is on the typed `Table`, next to `with_one`. `with_contained_one` and
`with_contained_many` each register a
[`ContainedRelation`](vantage_table::references::ContainedRelation) (built by
`ContainedRelation::one` or `ContainedRelation::many`), and `vista_contained()` describes them to
Vista as references of a contained kind,
[`Reference::contains_one`](vantage_vista::Reference::contains_one) or
[`Reference::contains_many`](vantage_vista::Reference::contains_many):

```rust
# use vantage_sql::sqlite::SqliteDB;
# use vantage_table::table::Table;
# use vantage_types::EmptyEntity;
# use vantage_vista::Reference;
# fn product_table(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
#     Table::new("product", db)
#         .with_id_column("id")
#         .with_column_of::<String>("name")
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let db = SqliteDB::connect("sqlite::memory:").await?;
let orders = Table::<SqliteDB, EmptyEntity>::new("order", db)
    .with_id_column("id")
    .with_column_of::<String>("lines") // the host column: declare it (see Sharp edges)
    .with_contained_many("lines", "lines", |db| {
        Table::new("lines", db)
            .with_column_of::<String>("line_id")
            .with_column_of::<i64>("quantity")
            .with_column_of::<String>("product")
            .with_one("product", "product", product_table) // a line can traverse out
    }, Some("line_id"));

assert_eq!(orders.vista_contained(), [Reference::contains_many("lines", "lines")]);
# Ok(())
# }
```

The closure builds the contained record's schema. It has the same shape as `with_one`'s target
builder, uses the same type system, and runs lazily, at traversal.

**The sub-Vista** is [`build_contained_vista`](vantage_vista::build_contained_vista). It holds the
column's records in memory, serves reads from them, and after every write re-serializes the whole
collection and hands it to a [`ContainedWriteback`](vantage_vista::ContainedWriteback). The
writeback (patch the parent row's host column) is the only persistence-specific part:

```rust
use std::sync::{Arc, Mutex};

use vantage_vista::prelude::*;
use vantage_vista::{Column, ContainedWriteback, Reference, VistaMetadata, build_contained_vista};

fn line(id: &str, quantity: i64) -> CborValue {
    CborValue::Map(vec![
        (CborValue::from("line_id"), CborValue::from(id)),
        (CborValue::from("quantity"), CborValue::from(quantity)),
    ])
}

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let schema = VistaMetadata::new()
    .with_column(Column::new("line_id", "string"))
    .with_column(Column::new("quantity", "int"))
    .with_id_column("line_id");
let host = CborValue::Array(vec![line("l1", 2), line("l2", 1)]);

// Stands in for "patch the parent row's host column".
let written = Arc::new(Mutex::new(None));
let sink = written.clone();
let writeback: ContainedWriteback = Arc::new(move |collection: CborValue| {
    *sink.lock().unwrap() = Some(collection);
    Box::pin(async { Ok(()) })
});

let lines = Reference::contains_many("lines", "lines");
let records = build_contained_vista(&lines, schema, Some(&host), Some(writeback), None)?;
assert_eq!(records.list_values().await?.len(), 2);

records.delete("l1").await?;
let collection = written.lock().unwrap().clone();
assert_eq!(collection, Some(CborValue::Array(vec![line("l2", 1)])));
# Ok(())
# }
```

**The skeleton** every driver shares is
[`Table::get_contained_ref`](vantage_table::table::Table::get_contained_ref). It builds the
contained schema from the closure, seeds the records from the parent row, wires the writeback to
`patch_value` on the parent table, and resolves traverse-out (`line → product`) through the
contained table's own relations. You pass it only what the driver knows: the parent row with its id
in your native id type, a `wrap` closure that turns a target `Table` into a `Vista` through your
factory, and the host column's codec.

### Wiring it into your shell

Two changes, both small:

- **Metadata.** Ask the table for its contained relations as well:
  `table.vista_metadata(VistaMetadataOptions { contained: true, .. })`. They join the joined
  relations in `metadata().references`, so consumers see one list.
- **`get_ref`.** A contained relation routes through the same method as a join. When the
  reference's `kind.is_contained()`, call `get_contained_ref` instead of `traverse_erased`. With a
  row, pass the row and its id; with `None`, pass `None`, and you get the bare target: the contained
  schema with no records and no write capabilities, since there's no parent row to write into.

The SQL drivers' shell is the worked source (`vantage-sql/src/sql_vista/refs.rs`). Their host
column is `TEXT` holding JSON, so the codec parses on the way in and serializes on the way out:

```rust
# use vantage_sql::sqlite::SqliteDB;
# use vantage_table::table::Table;
# use vantage_types::{EmptyEntity, PlainDialect, Record, cbor_to_json, json_to_cbor};
# use vantage_vista::prelude::*;
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let db = SqliteDB::connect("sqlite::memory:").await?;
# let orders = Table::<SqliteDB, EmptyEntity>::new("order", db.clone())
#     .with_id_column("id")
#     .with_column_of::<String>("lines")
#     .with_contained_many("lines", "lines", |db| {
#         Table::new("lines", db)
#             .with_column_of::<String>("line_id")
#             .with_column_of::<i64>("quantity")
#     }, Some("line_id"));
// The parent row as the shell read it: `lines` is TEXT holding JSON.
let order: Record<CborValue> = [
    ("id".to_string(), CborValue::from("o1")),
    ("lines".to_string(), CborValue::from(r#"[{"line_id": "l1", "quantity": 2}]"#)),
]
.into_iter()
.collect();

let factory_db = db.clone();
let lines = orders.get_contained_ref(
    "lines",
    Some((&order, "o1".to_string())),
    move |target| factory_db.vista_factory().from_table(target),
    |host| match host {
        CborValue::Text(json) => serde_json::from_str(json).ok().map(json_to_cbor),
        _ => None,
    },
    |collection| CborValue::Text(cbor_to_json(&PlainDialect, collection).to_string()),
)?;
assert_eq!(lines.list_values().await?.len(), 1);
# Ok(())
# }
```

Nothing is written here: the records come from the row, and only a write through `lines` would
patch `order.lines` on the database.

The contract test from Step 8 covers the wiring. `check_advertised` traverses every reference,
contained ones included, from a row and with `None`, so a shell that advertises `Traverse` and
forgets the contained branch fails it.

### Native vs JSON-blob

The codec is the one place native and JSON-blob backends differ.

**Native backends** (SurrealDB, MongoDB) store the host column as a real nested object or array. The
value arrives as `CborValue::Map` / `Array` already, and the writeback patches it back as it is
(SurrealDB `UPDATE … MERGE`, MongoDB `$set`). Decode is a clone; encode is the identity.

**JSON-blob backends** (SQL with no native nesting, the SQLite path above) store the collection as a
JSON string in a `TEXT` column: parse on read, serialize on write. The SQL drivers' decoder also
passes a `Map` / `Array` straight through, so the same code copes with a backend that parses JSON
columns natively (Postgres `jsonb`, MySQL `json`). There, declaring the host column as `TEXT` keeps
the round-trip a plain string and avoids binding a nested value on write, which is why Postgres and
MySQL share the SQLite implementation.

### Why not just flatten the keys?

You can, for *reading* a fixed shape: `inventory.stock` as a scalar column is fine when there's one
known field. It falls apart the moment the embedded data is a *collection* (an order has N lines,
not a fixed set), or the user needs to add and remove elements. A contained relation gives you a
record set with ids, not a bag of dotted columns.

### Why not model it as a foreign-key relation?

Because there's no other table to point at, and no foreign key to join on. The data is in the row.
Forcing it into `with_many` would mean inventing a synthetic table and a join the storage engine
can't honour. Traversal here is a column projection, not a query.

### Eager writeback

Every mutation on the sub-Vista patches the parent row immediately; there's no `flush`. The
sub-Vista and the parent row stay coherent at all times, and that's deliberate: batching belongs to
a higher layer (a UI's write queue), not the storage boundary. The cost is one parent patch per edit,
which is trivial for the small collections this targets.

The flip side: the whole collection is re-serialized and written each time. A contained relation is
for line items and embedded objects, not for a thousand-element array mutated in a tight loop.

### Sharp edges

**Declare the host column.** This is the bug every backend hits. If `lines` isn't a declared column,
your read path won't select or project it (SQL builds its `SELECT` from declared columns; MongoDB
projects from its column paths), so the parent row arrives without it and traversal sees an empty
collection. Declare it alongside the `with_contained_*` call.

**Positional ids shift.** With no id column, contained-many records are keyed by position (`"0"`,
`"1"`, …), and deleting element 0 renumbers the rest. Give the relation an id column
(`with_contained_many(…, Some("line_id"))`) when callers hold on to ids across mutations.

**Contained-one uses a fixed id.** A single embedded object is addressed as `"0"`.

**The writeback is not atomic with the contained record's own children.** A contained record can
traverse out (`line → product`), but each backend write is its own statement. That's the same
best-effort contract as nested insert: fine for these shapes, not a transaction.

### From YAML

Contained relations are declarable in a YAML spec too, in a `contained:` section next to
`columns:` and `references:`:

```yaml
name: order
columns:
  id: { type: string, flags: [id] }
  lines: { type: string }          # the host column: declare it so it's selected
sqlite:
  table: order
contained:
  lines:
    host_column: lines
    kind: contains_many            # or contains_one, the default
    id_column: line_id             # optional; omit for positional ids
    columns:
      line_id: { type: string }
      quantity: { type: int }
```

The factory lowers it through one generic helper,
[`Table::with_contained_specs`](vantage_table::table::Table::with_contained_specs), which calls your
driver's existing column builder on each contained column. Wiring it is one line in the factory's
spec-to-table code: `table = table.with_contained_specs(&spec.contained, build_column)?;`. The YAML
and code-first paths then register the same `ContainedRelation`.

**Limitation:** YAML-declared contained records carry *columns only*, so traverse-out
(`line.product`) isn't expressible from YAML yet; the code-first closure supports it, since it can
add `with_one` to the contained table. Lifting this means letting contained columns carry
`references:` sugar plus a resolver, the same machinery YAML foreign-key references need.

### Step 9 checklist

A backend supports contained relations once it has:

1. **Metadata** built with `VistaMetadataOptions { contained: true, .. }`, so the relations appear
   in `metadata().references`.

2. **`get_ref`** routing a contained reference to `Table::get_contained_ref`, passing the parent row
   and its native id (or `None` for the bare target), the factory's `wrap`, and the host codec:
   passthrough for native nesting, JSON parse and serialize for a `TEXT` column.

3. **YAML**: one `with_contained_specs` line in the factory's spec-to-table code.

4. **Tests** against a real backend, gated on `feature = "vista"`: declare a host column holding a
   collection (code-first *and* through `from_yaml`), traverse it, insert and patch through the
   sub-Vista, and re-read the parent row to prove the writeback landed. The Step 8 contract test
   covers the traversal with and without a row.

Native and JSON-blob backends differ only in two closures: how the host value comes in and how the
writeback goes out. Everything between is shared.
