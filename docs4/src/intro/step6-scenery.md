# Scenery — Reactive Views

Chapters 4–5 built the data pipeline: Vista wraps your table, Dio caches it and routes writes. But
consumers — data grids, dashboards, CLI tools, test harnesses — don't want to deal with cache
management and event subscriptions directly. They want an ordered row set, a single record, or a
computed value that stays up to date.

**Scenery** is that layer. Each Scenery is a reactive view onto a Dio that exposes a specific access
pattern:

- [`TableScenery`](vantage_diorama::TableScenery) — ordered, paginated rows
- [`RecordScenery`](vantage_diorama::RecordScenery) — a single record by id
- [`ValueScenery`](vantage_diorama::ValueScenery) — a computed scalar (count, sum, custom)

All three share the same reactivity mechanism: a [`Generation`](vantage_diorama::Generation) counter
that bumps whenever the view's state changes. Consumers subscribe via a `watch` channel and react on
each bump.

```admonish example title="Goals for this chapter"
By the end of this page you'll be able to:

1. Open a TableScenery and read rows by index
2. Use sequential mode (append-only) vs random-access mode (viewport-driven)
3. Open a RecordScenery for a single record
4. Open a ValueScenery for aggregates
5. Subscribe to changes via the Generation counter
```

---

## Generation — the reactivity primitive

Every Scenery carries a monotonically increasing `Generation(u64)`. When the underlying Dio
publishes an event that affects the view, the Scenery recomputes its state and bumps the counter.

```rust
let mut rx = scenery.subscribe();

// Block until something changes
let gen = rx.changed().await.unwrap();
println!("updated to generation {:?}", gen);
```

The `subscribe()` call returns a `watch::Receiver<Generation>` — a Tokio channel that holds the
latest value. Multiple subscribers share the same channel. If nothing changes, `changed()` blocks
indefinitely. If the Scenery is dropped, the channel closes.

```admonish info title="Watch, not stream"
`watch` only keeps the *latest* value. If three changes happen before your consumer reads, it
sees one bump — not three. This is intentional: consumers re-read the full state on each bump,
so intermediate states are wasted work.
```

---

## TableScenery — ordered rows

The most common view. Built from a Dio via a builder:

```rust
use vantage_diorama::scenery::SortDir;

let scenery = dio
    .table_scenery()
    .sort("name", SortDir::Asc)
    .page_size(50)
    .open()
    .await?;
```

The builder chains configuration, then `.open()` seeds the view from cache, spawns a reactor task,
and returns `Arc<dyn TableScenery>`.

### Reading rows

```rust
for i in 0..scenery.row_count() {
    if let Some(row) = scenery.row(i) {
        let name = &row.record["name"];
        println!("  {}: {:?}", i, name);
    }
}
```

Rows are accessed by index — a `BTreeMap<usize, Arc<EnrichedRecord>>` under the hood. Not every
index is populated: the map is sparse when only part of the dataset is loaded. `row(i)` returns
`None` for unloaded indices.

### Two loading strategies

TableScenery supports two modes, selected automatically based on which Lens callbacks you've
configured:

**Sequential mode** — append-only paging. Use when the master can only return "the next page"
(cursor APIs, append-only logs). No `total_provider` is registered. You call `request_load_more()`
and the cache grows page by page:

```rust
scenery.request_load_more();
// cache grows: 50 → 100 → 150 → ...
```

```admonish warning title="Sequential mode: no skipping"
In sequential mode, `set_viewport()` past the cache end **clamps** to whatever is already loaded
and emits `ViewportClamped`. You can't jump to row 900 — you have to call `request_load_more()`
repeatedly until the cache grows that far. This matches the reality of cursor-based backends:
they can't skip ahead either.
```

**Random-access mode** — viewport-driven sparse paging. Use when you know the total row count and
can fetch any range. Register `total_provider` and `on_load_chunk` on the Lens. Then:

```rust
scenery.set_viewport(200..250);
// on_load_chunk fires for the missing range
// rows arrive in the sparse map
```

`set_viewport()` triggers a fetch only for the portion not already cached. Scrolling back to a
previously loaded range is instant. You can jump to any row — the master supports it.

### Table size: `row_count`, `estimated_total`, `has_more`

These three methods behave differently per mode:

|                     | Sequential                                  | Random-access                               |
| ------------------- | ------------------------------------------- | ------------------------------------------- |
| `row_count()`       | Cache size (grows as you load more)         | Total from `total_provider` (known upfront) |
| `estimated_total()` | `None` (unknown until you hit the end)      | `Some(total)`                               |
| `has_more()`        | Always `true` (no total to compare against) | `loaded < total`                            |

In sequential mode, the table size is a mystery — the master only knows "next page". You load pages
until `on_load_chunk` returns a short page (fewer rows than `page_size`). At that point,
`estimated_total()` freezes to the cache size and `has_more()` flips to `false`.

In random-access mode, the total is known from the start — `total_provider` runs once at open.
Scrollbars size correctly before any data loads. The sparse map fills gaps on demand.

```admonish info title="Which mode do I get?"
It depends on your Lens configuration:

| Callback registered | Mode | Pagination |
|---------------------|------|------------|
| `total_provider` + `on_load_chunk` | Random-access | `set_viewport()` |
| `on_load_chunk` only | Sequential | `request_load_more()` |
| Neither | Cache-only | Whatever `on_start` seeded |

Check `master_capabilities()` to see what the view advertises: `can_fetch_page` → random-access,
`can_fetch_next` → sequential.
```

### Search and sort

```rust
scenery.set_search(Some("tart".into()));
scenery.set_sort(Some("price".into()), SortDir::Desc);
```

These trigger a full reload from cache. Search and sort are applied locally against the cached data
— they work even when the master backend doesn't support them (capability injection from chapter 5
in action).

---

## RecordScenery — single record

A reactive view onto one record by id:

```rust
let record = dio.record_scenery("42").await?;

match record.status() {
    RecordStatus::Fresh => {
        let row = record.record().unwrap();
        println!("name: {:?}", row.record["name"]);
    }
    RecordStatus::NotFound => println!("not in cache"),
    RecordStatus::Error(msg) => println!("error: {}", msg),
    _ => {}
}
```

The record reads from cache at creation time — no master fetch on miss. If the cache doesn't have
the row, status is `NotFound`. The reactor listens to the Dio's event bus and reloads when
`RecordChanged` or `Invalidated` arrives for that id.

If you already have the record (e.g. from a table row click), skip the cache read:

```rust
let record = dio.record_scenery_with("42", row_from_table);
```

---

## ValueScenery — aggregates

A reactive scalar computed from the cache:

```rust
use vantage_diorama::scenery::Aggregate;

let count = dio
    .value_scenery()
    .count()
    .open()
    .await?;

let total = dio
    .value_scenery()
    .sum("price")
    .open()
    .await?;
```

Built-in aggregates:

| Method                | Computes                        |
| --------------------- | ------------------------------- |
| `.count()`            | Total rows in cache             |
| `.count_where(conds)` | Rows matching field=value pairs |
| `.sum("field")`       | Sum of an integer field         |
| `.max("field")`       | Maximum of an integer field     |
| `.min("field")`       | Minimum of an integer field     |

All are computed locally against the cache — no database query. The reactor recomputes on every
`DioEvent` and bumps the generation if the value changed.

```admonish tip title="Custom aggregates"
For computations the built-in variants don't cover:

~~~rust
let expensive = dio
    .value_scenery()
    .custom(|dio| {
        let dio = dio.clone();
        async move {
            let rows = dio.cache().list_values().await?;
            let avg = /* your logic */;
            Ok(avg.into())
        }
    })
    .open()
    .await?;
~~~

The closure receives a `&Dio` — same pattern as Lens callbacks.
```

---

## Putting it together

```rust
use vantage_diorama::scenery::SortDir;

// Table view — ordered, paginated
let table = dio
    .table_scenery()
    .sort("name", SortDir::Asc)
    .page_size(50)
    .open()
    .await?;

println!("{} rows loaded", table.row_count());

// Single record
let rec = dio.record_scenery("42").await?;

// Live counter
let count = dio.value_scenery().count().open().await?;
let mut rx = count.subscribe();
loop {
    rx.changed().await?;
    println!("count: {:?}", count.value());
}
```

---

## What we covered

| Concept                                           | What it does                                                         |
| ------------------------------------------------- | -------------------------------------------------------------------- |
| [`TableScenery`](vantage_diorama::TableScenery)   | Ordered, paginated row view with sequential or random-access loading |
| [`RecordScenery`](vantage_diorama::RecordScenery) | Reactive single-record view by id                                    |
| [`ValueScenery`](vantage_diorama::ValueScenery)   | Reactive aggregate (count, sum, max, min, custom)                    |
| [`Generation`](vantage_diorama::Generation)       | Monotonic counter bumped on every state change                       |
| `subscribe()`                                     | Returns `watch::Receiver<Generation>` for reactive updates           |
| `set_viewport()`                                  | Request a row range for random-access loading                        |
| `request_load_more()`                             | Append the next page for sequential loading                          |
| [`Aggregate`](vantage_diorama::Aggregate)         | Enum of built-in and custom aggregate operations                     |
