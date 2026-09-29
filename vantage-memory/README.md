# vantage-memory

An in-memory [Vantage](https://github.com/romaninsh/vantage) datasource: a typed `TableSource`
(`MemoryDB`) and a Vista `TableShell` (`MemoryTableShell` / `MemoryVistaFactory`), both reading and
writing through one store, with native change events.

It is not a mock. `MockShell`, `MockTableSource` and `ImDataSource` keep their existing role in
tests elsewhere in the workspace; this crate is real storage that happens to live in the process —
data lives as long as the process runs, and it is meant as a production backend for things like
faker's simulated data and browsable dev/demo tables.

## Quick use

### The sync store

The store's core API is synchronous, so it can be called directly from a plain thread with no
async runtime:

```rust
use ciborium::Value as CborValue;
use vantage_memory::{MemoryStore, TableDef, Query, MemoryCondition};
use vantage_vista::FilterOp;

let store = MemoryStore::new();
let table = store.define("product", TableDef {
    id_column: "id".into(),
    indexed: vec!["category".into()],
    id_prefix: None,
});

let record: vantage_types::Record<CborValue> = [
    ("name".to_string(), CborValue::Text("Tea".into())),
    ("category".to_string(), CborValue::Text("drink".into())),
]
.into_iter()
.collect();

let id = table.insert(record)?;      // generates an id, or errors if the supplied one exists
table.patch(&id, &partial_record);   // merges fields; false if the id is missing
table.get(&id);
table.delete(&id);                   // false if the id is missing

let q = Query::new().filter(MemoryCondition::cmp(
    "category", FilterOp::Eq, CborValue::Text("drink".into()),
));
table.query(&q)?;
```

### The typed layer

```rust
use vantage_memory::MemoryDB;
use vantage_memory::prelude::*;
use vantage_table::prelude::*;

let db = MemoryDB::new();
let mut products = Table::new("product", db.clone())
    .with_id_column("id")
    .with_column_of::<String>("name")
    .with_column_of::<i64>("price")
    .with_column_of::<String>("category");

products.add_condition(products["price"].gt(3));
products.add_order(products["price"].descending());
let rows = products.data_source().list_table_values(&products).await?;
let total = products
    .data_source()
    .get_table_sum(&products, &products["price"])
    .await?;
```

### A Vista, from YAML

```rust
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_vista::VistaFactory;

let yaml = r#"
name: product
id_column: id
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [title, searchable] }
  price: { type: int }
  category: { type: string }
memory:
  indexed: [category]
  seed: seed/product.yaml
"#;

let vista = MemoryVistaFactory::new(MemoryStore::new()).from_yaml(yaml)?;
```

`memory.indexed` adds a hash index to each named column (applied even to a table that already
exists, as long as its id column agrees with the spec's). `memory.seed` points at a `.json`,
`.yaml` or `.yml` file of rows, loaded when the vista is built and the table is still empty, so
rebuilding a vista keeps edits made since. A relative seed path is resolved against the process
working directory, not the spec file's location.

### Live changes

```rust
let mut stream = vista.watch().await?;
while let Some(change) = stream.next().await {
    let change = change?; // VistaChange::{Inserted, Updated, Deleted, Invalidated}
}
```

A filtered vista sees a row that starts matching as `Inserted` and one that stops matching (by
edit or delete) as `Deleted`, never a bare update outside the filter. A lagging subscriber gets
`Invalidated` and is expected to re-list, as does every subscriber when a table leaves quiet mode
after writes were made while quiet.

The same stream is what a `Dio` (vantage-diorama) rides on: attach one over a memory vista and it
stays correct through `watch()` alone, with no separate event-forwarding wired up. A write that
lands between the Dio's initial load and its `watch()` call is not seen; start watching before the
load, or refresh once after it.

## Semantics

- **Comparison.** Integers and floats compare numerically across the two types (`1` and `1.0` are
  equal); text compares by code point; booleans compare `false < true`. Values of different kinds
  never match `Gt`/`Gte`/`Lt`/`Lte`.
- **Null.** A null cell matches `Eq null`, `Ne <non-null value>`, and `NotInSet` against a set
  holding no null. It fails every other op.
- **`Like`.** `%` (any run) and `_` (one character), case-insensitive, over the text form of the
  cell — numbers compare by their decimal form.
- **Search.** Case-insensitive substring over every text cell and the text form of every number.
- **Ordering.** Nulls sort first ascending and NaN after every other number; descending reverses
  the whole comparison, so nulls sort last there. Ties keep insertion order.
- **Joins.** Ids are stored as text, but a reference also matches an integer foreign key
  (`client_id: 1` finds client `"1"`), in both the Vista and typed layers.
- **Paths.** A path is a column name, or a dotted path into nested maps (`address.city`); a
  missing path reads as null. A column literally named with a dot wins over the dotted reading.
- **Ids.** A caller-supplied id (present in the record's id column) is used as given. Otherwise a
  per-table counter generates `"1"`, `"2"`, … or `"<prefix>1"`, skipping any id already taken by a
  supplied one.
- **No-op writes.** A `patch` that changes nothing, or an `upsert` that replaces a row with an
  identical one, sends no event and does not count toward `writes()`. A `patch`/`delete` on a
  missing id returns `false` and likewise sends nothing. A `patch` never changes the id column.
- **Atomic writes.** `insert_as`, `replace`, `upsert`, `patch` and `delete` each check, write and
  broadcast under one hold of the table's lock, so concurrent writers cannot slip in between.
- **Quiet mode.** `table.set_quiet(true)` stops event broadcast without changing what gets written
  or what `writes()` counts. `set_quiet(false)` sends one `MemoryChange::Reset` if anything was
  written meanwhile.

## Performance

`examples/bench.rs` times insert, patch, get, an indexed `Eq` query on a column with 5 distinct
values (`indexed_eq`, ~size/5 matches) and on one with ~size/10 distinct values
(`indexed_eq_selective`, ~10 matches), an unindexed range query (`n > size/2`), an ordered page
(order by `n` descending, window `(size/2, 50)`), and deleting rows from the front of the table,
at 1,000, 10,000 and 100,000 rows. It prints numbers and asserts nothing. Run it with:

```
cargo run -p vantage-memory --example bench --release
```

Numbers below are from one release run; treat them as relative, not absolute — a debug build is
noticeably slower and is fine for exercising the code path, not for reading timings off.

| size    | op                   | ops/s      | µs/op   |
|---------|----------------------|-----------:|--------:|
| 1,000   | insert               |  1,506,591 |    0.66 |
| 1,000   | patch                |  1,659,635 |    0.60 |
| 1,000   | get                  | 58,251,296 |    0.02 |
| 1,000   | indexed_eq           |    107,496 |    9.30 |
| 1,000   | indexed_eq_selective |  1,431,555 |    0.70 |
| 1,000   | range_scan           |     51,727 |   19.33 |
| 1,000   | ordered_page         |     31,377 |   31.87 |
| 1,000   | delete               |    695,048 |    1.43 |
| 10,000  | insert               |  1,914,608 |    0.52 |
| 10,000  | patch                |  1,858,132 |    0.54 |
| 10,000  | get                  | 27,118,644 |    0.04 |
| 10,000  | indexed_eq           |      8,315 |  120.27 |
| 10,000  | indexed_eq_selective |    802,998 |    1.25 |
| 10,000  | range_scan           |      4,847 |  206.31 |
| 10,000  | ordered_page         |      3,016 |  331.52 |
| 10,000  | delete               |     80,562 |   12.41 |
| 100,000 | insert               |  1,964,222 |    0.51 |
| 100,000 | patch                |  1,842,939 |    0.54 |
| 100,000 | get                  | 10,386,732 |    0.10 |
| 100,000 | indexed_eq           |        256 | 3910.41 |
| 100,000 | indexed_eq_selective |    477,033 |    2.10 |
| 100,000 | range_scan           |        227 | 4396.31 |
| 100,000 | ordered_page         |        173 | 5764.77 |
| 100,000 | delete               |      5,612 |  178.20 |

Insert and patch stay flat: a patch that leaves every indexed cell unchanged does no index work.
An indexed query costs what it returns — each candidate is sorted back into insertion order,
re-checked against every condition and copied out — so `indexed_eq_selective` stays in the
microseconds while `indexed_eq`, returning a fifth of the table, grows with it. `range_scan` and
`ordered_page` have no index to narrow with: they check every row, and `ordered_page` also sorts
the matches before windowing. `delete` is O(n): the row map shifts every later row down to keep
insertion order.

## Limits

- No persistence. Everything lives only as long as the process; `seed::load`/`seed::dump` are
  public so a snapshot format could be layered on later without a redesign.
- No transactions spanning more than one write.
- No `aggregate_vista`; the Vista layer keeps the trait's default (unimplemented) behavior.
- A `HasMany` child inserted through the reference's Vista does not have its foreign key stamped
  automatically, unlike the SQLite backend — set it on the record before inserting.
- Deleting a row is O(n) in the table size, to keep insertion order.

## License

MIT OR Apache-2.0
