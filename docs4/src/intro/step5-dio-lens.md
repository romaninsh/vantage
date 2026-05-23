# Dio & Lens — Caching and Events

Chapter 4 gave you Vista — a universal handle that works with any backend. But every call still hits
the database. No caching, no reactivity, no way to route writes elsewhere.

**Diorama** (`vantage-diorama`) sits between your Vista and whatever consumes it. Three things it
does:

1. **Transparent caching.** Keep a local copy of your data. Reads come from cache, not from the
   master — no matter how slow or rate-limited the backend is. A CSV file that takes 200ms to parse
   on every `list_values()` becomes instant after the first load.
2. **Capability injection.** A Vista backed by a CSV file can't paginate, sort, or search
   server-side. Diorama caches the full dataset locally and answers those queries from cache — the
   consumer sees a Vista that _can_ paginate, even though the underlying source can't.
3. **Custom write routing.** Writes don't have to go to the master. Route them to a Kafka topic, a
   queue, or a different database entirely. The cache updates immediately; persistence happens
   asynchronously.

```admonish example title="Goals for this chapter"
By the end of this page you'll be able to:

1. Build a Lens with cache backend and callbacks
2. Create a Dio from a Vista + Lens
3. Read from cache, write through the queue
4. React to live changes via the event bus
5. Produce a facade Vista that hides the caching layer
```

---

## The four words

Before the code, four terms you'll see throughout:

- **Vista** — a single backend data source (chapter 4). Speaks whatever the backend supports.
- **Lens** — long-lived shared infrastructure: cache backend, callbacks, refresh config. Built once.
- **Dio** — a Vista bound to a Lens. Owns the cache table, write queue, event bus, and refresh task.
  Produced by `lens.make_dio(vista)`.
- **Scenery** — a reactive view onto a Dio (tables, records, aggregates). The UI binds here. Covered
  in the next chapter.

The picture:

```text
Vista → Lens.make_dio(vista) → Dio → facade Vista | Scenery
                                      ↑
                                  cache + events
```

---

## Building a Lens

A [`Lens`](vantage_diorama::Lens) is built once and shared across every Dio you create from it. It
holds the cache backend, lifecycle callbacks, and default policies:

```rust
use std::sync::Arc;
use std::time::Duration;
use vantage_diorama::Lens;

let lens = Arc::new(
    Lens::new()
        .cache_at("./cache.redb")
        .on_start(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.master().list_values().await?;
                dio.cache().insert_values(rows).await?;
                Ok(())
            }
        })
        .refresh_every(Duration::from_secs(300))
        .build()?,
);
```

Three things happen here:

- **`.cache_at(path)`** — opens a redb file on disk. Each Dio claims a named table inside it. You
  can also pass a custom [`CacheBackend`](vantage_diorama::CacheBackend) with
  `.cache_source(backend)` for in-memory or remote stores.
- **`.on_start(|dio| { ... })`** — fires once when `make_dio` is called. The canonical pattern is
  seed the cache from the master: list everything, write it to cache. The `dio.clone()` inside the
  closure produces a `'static` future — required because the callback outlives the borrow.
- **`.refresh_every(duration)`** — schedules periodic refresh. Combined with `on_refresh` (another
  callback), the Dio re-fetches from the master on a timer.

```admonish info title="All callbacks borrow &Dio"
Every Lens callback receives `&Dio` — a borrowed reference to the Dio it's running for. If you
need to spawn a task or hold the Dio across an `.await`, clone it inside the closure:

~~~rust
.on_start(|dio| {
    let dio = dio.clone();
    async move {
        // dio is now owned, safe to hold across await
    }
})
~~~

This `clone()` is cheap — Dio wraps an `Arc`, so you're just bumping a reference count.
```

---

## Creating a Dio

[`make_dio`](vantage_diorama::Lens::make_dio) binds a Vista to the Lens. It opens the cache table,
spawns the write worker and refresh task, fires `on_start`, and returns a
[`Dio`](vantage_diorama::Dio):

```rust
let products_dio = lens.make_dio(products_vista).await?;
```

That one call does all of this:

1. Opens (or creates) a cache table named after the Vista
2. Spawns a write worker that drains the write queue
3. Starts the refresh timer (if `refresh_every` + `on_refresh` are set)
4. Fires `on_start` — your callback seeds the cache
5. Returns the Dio, ready to use

You can create many Dios from one Lens — one per entity, each with its own cache table and write
queue, all sharing the same cache file and callback configuration.

---

## Reading from the cache

The Dio's cache is a simple key-value store — `id → Record<CborValue>`. Reads come from cache, not
from the master:

```rust
// List everything in cache
let rows = products_dio.cache().list_values().await?;

// Get one record by id
let product = products_dio.cache().get_value("7").await?;

// Count
let n = products_dio.cache().count().await?;
```

The cache is intentionally dumb — no conditions, no sort, no search. It stores rows; query planning
lives on the Dio/Scenery layer.

```admonish tip title="The facade Vista"
Direct cache access works, but there's a more ergonomic path. `dio.vista()` returns a
[Vista](vantage_vista::Vista) backed by the Dio — reads go through cache, writes go through
the queue, schema comes from the master. Consumers can't tell the difference:

~~~rust
let mut v = products_dio.vista();
v.add_condition_eq("category_id", 1.into())?;
let rows = v.list_values().await?;  // served from cache
~~~

The facade's capabilities are the union of the master's and what the Lens provides. A read-only CSV
Vista gets `can_count` from the cache. If you register `on_write`, it gains `can_insert` too — the
queue accepts the write even though the master can't.
```

---

## Writing through the queue

Writes don't hit the master directly. They go into the Dio's write queue as a
[`WriteOp`](vantage_diorama::WriteOp):

```rust
use vantage_types::Record;

let record = Record::from_iter([
    ("name".into(), "Muffin".into()),
    ("price".into(), 175i64.into()),
]);

// Through the facade Vista
products_dio.vista().insert(&"muffin".to_string(), &record).await?;
```

Under the hood this enqueues a `WriteOp::Insert`. The write worker picks it up asynchronously and
either calls your `on_write` callback or applies it to `dio.master()` directly.

```admonish info title="Write-through vs write-around"
When `on_write` is **not** registered, the write worker applies ops to `dio.master()` directly —
the default write-through path. The insert returns as soon as the op is enqueued; the master write
happens in the background. If it fails, a `DioEvent::WriteFailed` is published on the event bus.

When `on_write` **is** registered, you control where writes go. A common pattern: write to Kafka
(via `on_write`) and let the Kafka consumer feed back through `on_event` to update the cache.
This decouples the write path from the read path entirely.
```

---

## The event bus

Every Dio has a `broadcast` channel. When data changes, events are published there:

```rust
use vantage_diorama::DioEvent;

let mut rx = products_dio.subscribe_events();

// In a spawned task
tokio::spawn(async move {
    while let Ok(event) = rx.recv().await {
        match event {
            DioEvent::RecordChanged { id } => {
                println!("updated: {}", id);
            }
            DioEvent::RecordInserted { id } => {
                println!("new: {}", id);
            }
            DioEvent::RecordRemoved { id } => {
                println!("deleted: {}", id);
            }
            DioEvent::Invalidated => {
                println!("full refresh happened");
            }
            _ => {}
        }
    }
});
```

Sceneries subscribe to this bus to react to changes. You can also subscribe directly — for logging,
metrics, cache warming, or triggering side effects in any consumer.

---

## Live updates: `on_event`

The event bus carries _internal_ events (cache writes, refreshes). But what about changes happening
on the master that you didn't initiate — other users editing records, a database trigger firing, a
webhook arriving?

`on_event` receives _upstream_ [`ChangeEvent`](vantage_diorama::ChangeEvent) objects and reconciles
them into the cache. Add it to the Lens alongside `on_start`:

```rust
let lens = Arc::new(
    Lens::new()
        .cache_at("./cache.redb")
        .on_start(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.master().list_values().await?;
                dio.cache().insert_values(rows).await?;
                Ok(())
            }
        })
        .on_event(|dio, event| {
            let dio = dio.clone();
            async move {
                match event {
                    ChangeEvent::Updated { id, new: Some(row) } => {
                        dio.patched(id, row).await?;
                    }
                    ChangeEvent::Deleted { id } => {
                        dio.cache().delete_value(&id).await?;
                        dio.invalidate_record(id);
                    }
                    ChangeEvent::Invalidated => {
                        dio.invalidate_all();
                    }
                    _ => {}
                }
                Ok(())
            }
        })
        .build()?,
);
```

The typical wiring: a live stream from SurrealDB, a Kafka consumer, or a WebSocket listener feeds
[`ChangeEvent`](vantage_diorama::ChangeEvent)s into `dio.handle_event(evt).await`, which invokes
your `on_event` callback. You write the row to cache and publish the matching `DioEvent` — any
subscriber (Scenery, logger, metrics) picks it up.

```admonish tip title="patched() andinvalidate_record()"

- `dio.patched(id, record)` writes to cache **and** publishes `DioEvent::RecordChanged`. The
  canonical "external system told us about a row" pattern.
- `dio.invalidate_record(id)` publishes `RecordChanged` **without** touching cache — use when you
  know the cache is already stale and a Scenery will refetch.
- `dio.invalidate_all()` publishes `Invalidated` — Sceneries respond by re-reading their full state.

```

---

## Manual refresh

`on_refresh` fires on the timer set by `refresh_every`. You can also trigger it manually:

```rust
products_dio.refresh().await?;
```

This fires the `on_refresh` callback (if registered) and publishes `Invalidated` on the event bus.
Sceneries re-read from cache after the refresh completes.

---

## Callback summary

| Callback         | When it fires              | Typical use                          |
| ---------------- | -------------------------- | ------------------------------------ |
| `on_start`       | Once at `make_dio`         | Seed cache from master               |
| `on_refresh`     | Timer + manual `refresh()` | Re-fetch from master                 |
| `on_write`       | Every `WriteOp`            | Custom write routing (e.g. to Kafka) |
| `on_event`       | Upstream `ChangeEvent`     | Reconcile live updates into cache    |
| `on_query`       | Scenery data fetch         | Custom query routing                 |
| `total_provider` | Scenery open               | Supply total row count               |
| `on_load_chunk`  | Scenery viewport/page      | Fetch a range from master            |

---

## Putting it together

Everything in one place:

```rust
use std::sync::Arc;
use std::time::Duration;
use vantage_diorama::Lens;

// Build once per application
let lens = Arc::new(
    Lens::new()
        .cache_at("./cache.redb")
        .on_start(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.master().list_values().await?;
                dio.cache().insert_values(rows).await?;
                Ok(())
            }
        })
        .refresh_every(Duration::from_secs(300))
        .build()?,
);

// One Dio per entity
let products = lens.make_dio(products_vista).await?;

// Read and write through the facade — cache is transparent
let mut v = products.vista();
let rows = v.list_values().await?;       // cache hit
v.insert(&"muffin".to_string(), &rec).await?;  // enqueued
```

---

## What we covered

| Concept                                             | What it does                                                  |
| --------------------------------------------------- | ------------------------------------------------------------- |
| [`Lens`](vantage_diorama::Lens)                     | Shared infrastructure: cache, callbacks, defaults             |
| [`LensBuilder`](vantage_diorama::LensBuilder)       | Chainable configuration for building a Lens                   |
| [`Dio`](vantage_diorama::Dio)                       | Per-entity binding of Vista + Lens; owns cache, queue, events |
| [`CacheBackend`](vantage_diorama::CacheBackend)     | Storage backing for cached rows                               |
| [`WriteOp`](vantage_diorama::WriteOp)               | One unit of work on the write queue                           |
| [`DioEvent`](vantage_diorama::DioEvent)             | Internal event: record changed, invalidated, write failed     |
| [`ChangeEvent`](vantage_diorama::ChangeEvent)       | Upstream event from the master backend                        |
| `on_start` / `on_refresh` / `on_write` / `on_event` | Lifecycle callbacks                                           |
| `dio.vista()`                                       | Facade Vista that reads from cache, writes through queue      |

```admonish tip title="What's next"
Dio gives you caching and events. But consumers often need more — ordered rows, viewport-driven
loading, aggregate counts. The next chapter introduces **Scenery**: reactive views that sit on top
of a Dio and provide structured access patterns for tables, individual records, and aggregates.
```
