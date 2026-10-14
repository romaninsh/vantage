# Vantage Diorama

A Vista is a window onto one data source: a CSV file, a SQL table, an S3
bucket, a REST endpoint. Each source has its quirks. Some can't sort, some
are slow, some go offline, some page and some don't. An application that
talks to the Vista directly inherits every one of those quirks, in every
screen and every handler.

Diorama sits between the Vista and the application. It keeps a local copy of
the data, answers reads from it, sends writes through it, and tells anything
watching when the data moves. The application sees one shape for every
source; the differences live in one place, the Lens.

## The four words

- **Vista**: one backend source, from `vantage-vista`. It advertises exactly
  what the backend can do natively.
- **Lens**: the policy. Where rows are cached, how the cache is filled, how
  often it is re-read, where writes go. Built once and shared.
- **Dio**: one Vista run through a Lens. It owns the cache table, the write
  queue and the event bus for that one set of rows.
- **Scenery**: a reactive view onto a Dio. A table, a single record, or a
  single value. A UI binds to it and repaints when its generation moves.

The flow is `Vista → lens.dio(vista).build() → Dio → table_scenery() /
record_scenery(id) / value_scenery(aggregate)`.

## Example

```rust
use ciborium::Value as CborValue;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::aggregate::Count;
use vantage_diorama::{Cache, ChangeFlash, Lens, Loader};
use vantage_types::Record;
use vantage_vista::{Column, SortDirection, Vista, VistaMetadata, mocks::MockShell};

let metadata = VistaMetadata::new()
    .with_column(Column::new("id", "String").with_flag("id"))
    .with_column(Column::new("name", "String"))
    .with_column(Column::new("price", "i64"))
    .with_id_column("id");
let mut espresso = Record::new();
espresso.insert("name".to_string(), CborValue::Text("Espresso".into()));
espresso.insert("price".to_string(), CborValue::Integer(3.into()));
let master = Vista::new(
    "products",
    MockShell::new().with_metadata(metadata).with_record("p1", espresso),
);

let lens = Lens::builder()
    .cache(Cache::memory())
    .loader(Loader::eager(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    }))
    .build()?;

let products = lens.dio(master).build().await?;

let grid = products
    .table_scenery()
    .order("price", SortDirection::Ascending)
    .open()
    .await?;
let count = products.value_scenery(Count::rows()).open().await?;

let mut rename = Record::new();
rename.insert("name".to_string(), CborValue::Text("Ristretto".into()));
products.flash(ChangeFlash::partial("p1", rename)).await?;
```

The Lens is the part worth thinking about. It decides what is cached, when it
is refreshed and where writes land. Every Dio built from it inherits that
policy, and the sceneries opened on each Dio are cheap to open and cheap to
drop. The flash stages the change in the cache at once, so `grid` shows it
before the master confirms, and rolls it back if the master refuses.

`products.cached()` gives the same Dio back as an ordinary `Vista` for code
that doesn't care about reactivity: a handler, a CLI, a test. Reads come
from the cache and writes go through the same queue as a flash.

## Prelude

`use vantage_diorama::prelude::*;` brings in `Aggregation`, `ChangeEvent`,
`ChangeFlash`, `Dio`, `Lens`, `RecordScenery`, `Servo`, `TableScenery` and
`ValueScenery`. The Lens settings (`Cache`, `Loader`, `LoaderCx`,
`Refresher`, `Tuning`) and the aggregates under `vantage_diorama::aggregate`
are imported by name: a file that builds a Lens says so in its imports.

## Features

- `diagnostics`: the debug stream (`LensBuilder::debug_datasource` and the
  `vantage_diorama::debug` tracing target), live-instance counts in
  `vantage_diorama::stats`, and `Dio::diagnostics`. The tap runs in every
  build; the feature publishes it.
- `test-util`: hooks for harnesses that watch a Dio's internals, such as
  `Dio::take_write_worker_handle`.
- `rhai`: `Servo` as a Rhai type, registered with
  `vantage_diorama::rhai::register_servo_onto`.

## Where to go next

The book walks through Diorama with running code:

- [A facade over a custom API](https://vantage-ui.com/book/intro/step5-dio-lens.html):
  the first Lens and Dio.
- [Augmentation](https://vantage-ui.com/book/augmentation.html): enriching
  each row from a second source.
- [Live data](https://vantage-ui.com/book/live-data.html): push, poll and
  freshness.
- [Optimising Diorama](https://vantage-ui.com/book/optimising-diorama.html):
  the debug stream, capabilities and latency.
- [Migrating to 2.0](https://vantage-ui.com/book/migration-2.0.html): every
  removed name and what replaced it.

The guides in this crate go deeper on one role each:

- [Configuring a Lens](https://github.com/romaninsh/vantage/blob/main/vantage-diorama/README_lens.md):
  the four settings, cache identity, events.
- [Implementing a data source](https://github.com/romaninsh/vantage/blob/main/vantage-diorama/README_datasource.md):
  what a driver and a loader each owe Diorama.
- [Working on Diorama in Rust](https://github.com/romaninsh/vantage/blob/main/vantage-diorama/README_rust_dev.md):
  using a Dio from business code, the module map, features, tests and the API snapshot.
- [Wiring up a UI](https://github.com/romaninsh/vantage/blob/main/vantage-diorama/README_ui.md):
  binding sceneries to widgets.
- [Defining models](https://github.com/romaninsh/vantage/blob/main/vantage-diorama/README_models.md):
  the column metadata that helps Diorama, and aggregates.
- [Architecture](https://github.com/romaninsh/vantage/blob/main/vantage-diorama/ARCHITECTURE.md):
  protocols and concurrency, for maintainers.
