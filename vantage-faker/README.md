# vantage-faker

Synthetic data for Vantage: column generators that seed `vantage-memory` tables, and Rhai sims
that mutate them live.

> Incubating: API may change.

## Generators and `DatasetGen`

A `FakerColumn` names a column, its declared type and free-form flags (`"id"`, …), with an
optional `ColumnGen` override. `ValueGen` decides what a cell contains: the generator if one is
set, else the column name (`email`, `name`, `city`, …), then the declared type as a fallback.
`ColumnGen` covers `pick` (weighted choice), `range`, `date` (relative bounds, random or even
spread), `sentence`, `pattern` (`BA####`) and the positional `walk` (a time-series random walk) and
`tree` (same-table parent links).

`TableGen` declares one table — its columns, row count, id column and any reference columns
pointing at another table in the same `DatasetGen`, optionally with a `FanOut` giving each parent
a bounded, contiguous run of children. `DatasetGen::generate` seeds every declared table into a
`vantage_memory::MemoryStore`, referenced tables before the tables that reference them, quietly —
each table sends a single `Reset` once its rows are in, rather than one change per row.

```rust
use vantage_faker::{ColumnGen, DatasetGen, FakerColumn, TableGen};
use vantage_memory::MemoryStore;

let store = MemoryStore::new();
DatasetGen::new(Some(42))
    .table(TableGen::new("client").column(FakerColumn::new("name", "string")).count(5))
    .table(
        TableGen::new("invoice")
            .column(FakerColumn::new("client_id", "string"))
            .column(FakerColumn::new("status", "string").with_generator(ColumnGen::Pick {
                values: vec!["Open".into(), "Paid".into()],
                weights: None,
            }))
            .reference("client_id", "client")
            .count(20),
    )
    .generate(&store)
    .unwrap();
```

A `seed` makes the whole dataset reproducible; `None` draws fresh entropy per table.

## Sims

The `sim` feature adds `SimEngine`: a set of Rhai scripts that mutate a `MemoryStore`'s tables
live, one thread per running sim. A `SimDef` names a script, the table it writes to by default, a
`Spawn`er (`burst`, `rate_per_min`, `max`), a clock speed and an optional warm start:

```rust,ignore
use vantage_faker::{SimDef, SimEngine};

let engine = SimEngine::builder()
    .store(&store)
    .sim(SimDef::new("shipment", "shipments", include_str!("shipment.rhai")))
    .seed(42)
    .start()
    .unwrap();
```

Each live sim runs its script top to bottom on its own small thread; the script's locals are its
state, and `sleep(d)` / `wait_until(t)` block it until its own sim clock reaches the target. The
sim ends when the script does, when it calls `done()`, or on its first Rhai error — a thrown
exception, or the operations budget (`SimDef::with_ops`, default `DEFAULT_OPS`) run out between two
sleeps. A failed sim just ends; it doesn't stop the others or the engine.

Scripts write through data verbs, `table` optional everywhere (defaults to the def's table):

| verb | does |
|---|---|
| `insert(table?, #{…}) -> id` | new row; an explicit id that already exists is a script error |
| `upsert(table?, id, #{…})` | insert or replace |
| `patch(table?, id, #{…})`, `set(table?, id, field, value)` | change a row; missing row is ignored |
| `delete(table?, id)` | remove a row |
| `get(table?, id) -> map or ()` | read one row |
| `ids(table?)`, `count(table?)` | every id, or how many |
| `find(table?, #{col: value, …}) -> [id]` | ids matching every entry, in insertion order |

A def with `warm: Some(d)` runs `d` of sim time before the engine goes live, so a table opens
mid-life instead of empty. Every store table stays quiet for the whole warm start; a table written
during it broadcasts one `Reset` when the start ends, instead of one change per row.

## `ShapedShell`

`ShapedShell` wraps any `vantage_vista::source::TableShell` to make an in-memory table behave like
a real backend: paged or cursor-driven, sluggish or flaky, honest or lying about its counts. A
`BackendShape` is the whole personality — the `VistaCapabilities` it advertises (everything else
is refused), a `LatencyModel` per operation class, a `FaultSchedule` (error rate, scheduled
outages, cursor expiry, boundary skew) and a `seed` that replays the same draws. See
`tests/shaped_backends.rs` for the full contract.

## Features

- default — generators, `DatasetGen`, `ShapedShell`.
- `sim` — the Rhai sim engine (pulls in `vantage-rhai`).

## License

MIT OR Apache-2.0
