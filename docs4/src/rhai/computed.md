# Computed Columns

A computed column is a column whose value no backend stores. It is a short Rhai expression over
`row`, the record as the backend returned it, and its last expression is the column's value. The
expression lives in the Vista's metadata, and the Vista fills the column on every read, so it works
the same over SQLite, SurrealDB, a REST API, a CSV file or a memory store.

In YAML it is the `lazy:` key on a column. This chapter covers what a computed column does, how to
declare one, what it refuses, and how relations treat it.

<!-- toc -->

---

## An example

The examples run over two memory tables built from YAML. An order stores an invoice `code` and a
`net` amount. Its client key is buried in the code, and its tax and gross amounts are derived:

<!-- tested: rhai_guide::computed::declare_in_yaml (the fixture of every rhai_guide::computed test) -->
```yaml
name: order
columns:
  id: { type: string, flags: [id] }
  code: { type: string }
  net: { type: int }
  client:
    type: string
    references: client
    lazy: |
      let parts = row.code.split("-");   // "INV-c1-0001"
      parts[1]
  vat: { type: int, lazy: "row.net / 5" }
  gross: { type: int, lazy: "row.net + row.vat" }
```

<!-- tested: rhai_guide::computed::declare_in_yaml (the fixture of every rhai_guide::computed test) -->
```yaml
name: client
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
  vip: { type: bool }
  badge:
    type: string
    lazy: |
      if row.vip { "★ " + row.name } else { row.name }
references:
  orders: { table: order, kind: has_many, foreign_key: client }
```

A script reads computed columns like any other:

<!-- tested: rhai_guide::computed::reads -->
```rhai
let o1 = table("order").get("o1");

#{
    o1: [o1.client, o1.vat, o1.gross],
    badges: table("client").list().map(|c| c.badge),
    computed: table("order").columns()
        .filter(|c| c.flags.contains("calculated"))
        .map(|c| c.name),
}
```

For order `INV-c1-0001` with `net: 100` this returns `o1: ["c1", 20, 120]`, the badges
`["★ Ada", "Ben"]`, and `computed: ["client", "vat", "gross"]`.

## How the value is computed

- Columns compute in declaration order, and each sees the values of those declared before it.
  `gross` reads `row.vat`, which `vat` has just added.
- `row` is the record as the backend returned it. Nested records keep their shape
  (`row.Message.Headers.Subject`), and a missing field is `()`. The id column may not be in the
  record: memory and several other backends keep the id as the row's key, not as a field.
- The script is a block: `let` statements and helper expressions are fine, and the last expression
  is the value.
- It compiles once, when the column is declared. A script that doesn't parse fails the table build,
  not the first read. A script that throws on a row fails that read with "Computed column failed",
  naming the column.
- It runs under the background operation limit (50 million), so a runaway loop errors instead of
  hanging the read.
- There is no `table(name)` in scope. A computed column sees one row. To bring in data from another
  table, use [augmentation](../augmentation.md).

Every read path fills computed columns: `list_values`, `get_value`, `get_some_value`, streams,
`fetch_page`, `fetch_next`, `fetch_window`, and watched changes. Anything built on a Vista sees them
without knowing they exist: data scripts, Dio caches, grids.

## Declaring one

In YAML, `lazy:` goes on the column, next to `type:` and `flags:`. Every driver's factory lowers it
through [`ColumnSpec::lazy_column`](vantage_vista::ColumnSpec::lazy_column). The script can be
inline, as above, or in a file. A loader that supports `!include`, as Vantage UI's does, replaces
`lazy: !include ../script/client_key.rhai` with the file's text before the spec is parsed, so
`vantage-vista` only ever sees the script.

In Rust, [`Column::with_expression`](vantage_vista::Column::with_expression) turns a column into a
computed one:

<!-- tested: rhai_guide::computed::declare_in_rust -->
```rust,ignore
let gross = Column::new("gross", "int")
    .with_flag("orderable")
    .with_expression("row.net + row.vat")
    .unwrap();

assert!(gross.is_computed());
assert_eq!(gross.expression(), Some("row.net + row.vat"));
assert!(gross.has_flag(flags::CALCULATED));
assert!(!gross.has_flag(flags::ORDERABLE));
```

`with_expression` needs the `rhai` feature and returns an error, naming the column, when the
script doesn't compile. It flags the column `calculated` and removes `orderable` and `searchable`.
A shell then reports the column in its `VistaMetadata` like any other. Typed tables take spec
columns through `Table::add_spec_column`, which registers a `lazy:` column as computed. That is how
the SQL, SurrealDB, MongoDB, CSV, REST and GraphQL factories fold `lazy:` into a Vista.

The expression isn't serialized. A `Column` that goes through serde comes back as a stored column
that keeps only its `calculated` flag.

If you only need the closure, [`lazy_value_closure`](vantage_vista::rhai::lazy_value_closure)
compiles a script into an `Fn(&Record) -> Result<CborValue>`:

<!-- tested: rhai_guide::computed::closure -->
```rhai
row.contents.split("\n").len() - 1
```

## What they refuse

The backend holds no values for a computed column, so anything that would ask the backend about it
is refused.

**Writes drop them.** Inserts, patches, upserts and imports remove computed columns from the record
before it reaches the backend:

<!-- tested: rhai_guide::computed::writes -->
```rhai
let orders = table("order");
orders.insert(#{ id: "o4", code: "INV-c2-0004", net: 50, gross: 1 });
orders.patch("o1", #{ net: 200, vat: 0 });

[orders.get("o4").gross, orders.get("o1").vat]
```

This returns `[60, 40]`: the `gross: 1` and `vat: 0` were dropped, and the read computed both
from the new `net`. Vantage UI keeps computed columns out of its forms for the same reason.

**Filters, orders and aggregates error**, naming the column:

<!-- tested: rhai_guide::computed::refused -->
```rhai
table("order").where("gross", ">", 100).count()   // throws
```

<!-- tested: rhai_guide::computed::refused -->
```rhai
table("order").sort("gross", "desc").list()   // throws
```

The first fails with `Step can't be applied (step: Where { col: "gross", … }): Computed column
can't be used to filter, order or aggregate; the backend doesn't hold its values (column: "gross",
operation: "condition")`. The error is marked unsupported, like any other narrowing a backend can't
do. In Rust, `Vista::add_condition`, `add_condition_eq` and `add_order` refuse the same way, and so
does `aggregate` for a computed column or group key:

<!-- tested: rhai_guide::computed::aggregate_refused -->
```rust,ignore
let sum = orders.aggregate(&AggregateSpec::new("sum", "total").column("gross"));
assert!(sum.is_err());
let by_client = orders.aggregate(&AggregateSpec::new("count", "n").group_by("client"));
assert!(by_client.is_err());
```

To filter on a derived value, filter on the stored columns it comes from (`net` rather than
`gross`), or compute it in the backend's query: a query-sourced table or SurrealDB's `expr:`
column (see [Expression dialects](./dialects.md)).

## Relations through a computed column

A has-one relation whose foreign key is a computed column of the parent works. The parent row
already carries the computed value, and the target is narrowed by its own id, which it stores.
`order.client` above is such a key. A has-many relation whose foreign key is a computed column of
the child can't work: the child table would have to be filtered by a value it doesn't store.

<!-- tested: rhai_guide::computed::relations -->
```rhai
let big = table("order").where("net", ">", 50);

#{
    one: table("order").where("id", "o3").ref("client").first().name,
    many: big.ref("client").sort("name").list().map(|c| c.name),
}
```

Both forms of `ref` work for has-one: from one row (`"Ben"`), and from several (`["Ada", "Ben"]`),
where the parents' computed keys become an `in` condition on the client id. The reverse direction
throws:

<!-- tested: rhai_guide::computed::relations -->
```rhai
table("client").where("id", "c1").ref("orders").ids()   // throws
```

with `Has-many relation can't join on a computed column of its target (relation: "orders", column:
"client")`. `Vista::get_ref` makes the same check. A nested insert that would have to write a
computed foreign key (into the parent for has-one, into each child for has-many) is refused before
anything is written.

## `lazy:` and `sugar:`

Vantage UI has a second way to add derived columns: `sugar:` on a grid, binder or other component.
The two look similar and are used for different things.

| | `lazy:` (computed column) | `sugar:` (Vantage UI) |
|---|---|---|
| Declared on | the table | one component on one page |
| Seen by | every reader of the Vista: pages, relations, data scripts, Dio caches | that component only |
| Runs | inside the Vista, on each read | on its own thread, over the rows the component reads |
| Script shape | one expression over `row` | a loop over `next_row()` that calls `emit(#{ … })`, with helper functions and setup before the first row |
| Relations | has-one can follow it | none |

Use `lazy:` for a value that is part of the data, such as a key, an amount or a status that
relations and scripts need. Use `sugar:` for presentation on one page, or when the script needs
setup (a lookup map, helpers) before the first row. Sugar is built on
diorama's [`Sugar`](vantage_diorama::Sugar), which merges the outputs into the rows one
Scenery reads and strips them from anything written back. Its YAML is documented in Vantage UI's
skills.

The third look-alike is the typed table's `with_lazy_expression`, an async Rust closure per row
(the intro's [Augmentation step](../intro/step6-augmentation.md) uses it to download a file and
derive columns from it). It runs inside the typed table, before the Vista sees the row, and it can
do I/O, which a `lazy:` script can't. The Vista reports its column as `calculated`.
