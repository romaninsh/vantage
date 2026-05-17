# Diorama Architecture

This document describes the internal structure of `vantage-diorama` — the
trait surfaces, the type relationships, the concurrency model, and the rules
that govern how callbacks fire and capabilities propagate. It's the reference
for anyone maintaining the crate or writing adapters that plug into it.

The role-specific READMEs (`README_lens.md`, `README_ui.md`, etc.) cover the
public-facing surface. This file covers the rest.

## Layer diagram

```
+----------------------------------------------------------+
|                       Application                        |
|                                                          |
|  vista() — snapshot reads/writes   scenery() — reactive  |
+----------------------------------------------------------+
                  ▲                          ▲
                  │                          │
+----------------------------------------------------------+
|                          Dio                             |
|                                                          |
|  master: Vista (low-cap)     cache: Vista (lens-backed)  |
|  write queue (mpsc)          event bus (broadcast)       |
|  refresh task                                            |
+----------------------------------------------------------+
                            ▲
                            │
+----------------------------------------------------------+
|                          Lens                            |
|                                                          |
|  cache_source: Arc<dyn TableSource>   (redb default)     |
|  callbacks: Arc<LensCallbacks>        (HRTB async)       |
|  default policies (TTL, retries, refresh interval)       |
|  runtime: tokio::Handle                                  |
+----------------------------------------------------------+
                            ▲
                            │
+----------------------------------------------------------+
|                    Storage / Network                     |
|                                                          |
|  redb file       moka hot tier      remote Vistas        |
+----------------------------------------------------------+
```

A single `Lens` is shared by many `Dio`s. A single `Dio` produces many
short-lived `Vista` and `Scenery` handles. Storage is shared at the `Lens`
level (one redb file backs all Dios under that Lens).

## Lens

A Lens is configured once and built into an immutable handle. After build, the
Lens accepts `make_dio(vista)` calls and never mutates its own configuration.

```rust
pub struct Lens {
    cache_source: Arc<dyn TableSource>,
    callbacks: Arc<LensCallbacks>,
    defaults: LensDefaults,
    runtime: tokio::runtime::Handle,
}

pub struct LensBuilder {
    cache_source: Option<Arc<dyn TableSource>>,
    on_start:   Option<DioCallback>,
    on_refresh: Option<DioCallback>,
    on_write:   Option<DioWriteCallback>,
    on_event:   Option<DioEventCallback>,
    on_query:   Option<DioQueryCallback>,
    defaults: LensDefaults,
}

pub struct LensDefaults {
    pub refresh_interval: Option<Duration>,
    pub cache_ttl: Option<Duration>,
    pub write_queue_capacity: usize,        // default 256
    pub on_start_blocking: bool,            // default true — block make_dio until on_start completes
}
```

### Callback signatures

Callbacks borrow `&Dio` and return a future that may borrow from it. This is
the HRTB pattern; the closure itself is `'static` but the returned future is
not. Storing many different closures with this shape requires boxing.

```rust
pub type DioCallback = Box<
    dyn for<'a> Fn(&'a Dio) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>
    + Send + Sync,
>;

pub type DioWriteCallback = Box<
    dyn for<'a> Fn(&'a Dio, WriteOp) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>
    + Send + Sync,
>;

pub type DioEventCallback = Box<
    dyn for<'a> Fn(&'a Dio, ChangeEvent) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>
    + Send + Sync,
>;

pub type DioQueryCallback = Box<
    dyn for<'a> Fn(&'a Dio, QueryDescriptor) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>
    + Send + Sync,
>;
```

A `LensBuilder::on_start(F)` accepts any `F: for<'a> Fn(&'a Dio) -> Fut + …`
where `Fut: Future<Output = Result<()>> + Send + 'a` and wraps it into the
boxed shape above.

### Cache backend

`cache_source` is an `Arc<dyn TableSource>` — the same trait Vista drivers
implement. The default is redb; users can substitute anything that implements
`TableSource` (in-memory, sqlite, even another remote backend if they really
want).

The Lens treats this source as a multi-table backend: each Dio gets its own
table within it. The table name comes from `master.name()` by default, with
an override via `make_dio_named(name, vista)`.

## Dio

```rust
pub struct Dio {
    inner: Arc<DioInner>,
}

struct DioInner {
    lens: Arc<Lens>,
    master: Vista,
    cache: Vista,                              // built on construction
    cache_table_name: String,
    write_queue: mpsc::Sender<WriteOp>,
    event_bus: broadcast::Sender<DioEvent>,
    refresh_task: Mutex<Option<JoinHandle<()>>>,
    write_worker: Mutex<Option<JoinHandle<()>>>,
    hot_tier: Arc<HotTier>,                    // moka cache for active rows
}
```

`Dio` is `Clone` (cheap — clones the `Arc`). Sceneries hold `Arc<DioInner>`
indirectly through their own state. The `Arc` keeps all per-Dio infrastructure
alive as long as any handle outlives the original `Dio`.

### Lifecycle

1. `lens.make_dio(vista)` constructs `DioInner` with empty queues, the master
   vista, and a fresh cache vista pointing at the lens's `cache_source` with
   table `vista.name()`.
2. The Lens spawns the write worker task and the refresh task.
3. If `on_start` is registered, the lens fires it. By default `make_dio`
   awaits the callback (`on_start_blocking = true`); set `false` to make it
   fire-and-forget.
4. The Dio is returned. Callers can immediately call `.vista()`, `.scenery()`.

### Dio public surface

```rust
impl Dio {
    pub fn vista(&self) -> Vista { /* DioShell-backed */ }
    pub fn table_scenery(&self) -> TableSceneryBuilder { /* ... */ }
    pub fn record_scenery(&self, id: impl Into<RecordId>) -> Arc<dyn RecordScenery> { /* ... */ }
    pub fn value_scenery(&self) -> ValueSceneryBuilder { /* ... */ }

    pub fn master(&self) -> &Vista { &self.inner.master }
    pub fn cache(&self) -> &Vista { &self.inner.cache }

    pub async fn refresh(&self) -> Result<()> { /* fires on_refresh */ }
    pub fn invalidate_record(&self, id: impl Into<RecordId>) { /* publishes event */ }
    pub fn invalidate_all(&self) { /* publishes event */ }
    pub fn patched(&self, id: impl Into<RecordId>, record: Record<CborValue>) {
        // user-driven patch: writes to cache, publishes event
    }
}
```

## DioShell — TableShell impl

The Vista returned by `dio.vista()` is a plain `vantage_vista::Vista`. Its
internal `Box<dyn TableShell>` is `DioShell`, which routes reads through the
cache and writes through the Dio's write queue.

```rust
struct DioShell {
    dio: Arc<DioInner>,
}

impl TableShell for DioShell {
    async fn list_vista_values(&self, ...) -> Result<Vec<Record<CborValue>>> {
        // 1. Try cache first.
        // 2. If lens.callbacks.on_query is registered AND cache is cold for this query,
        //    fire on_query(dio, descriptor) and re-read cache.
        // 3. Return rows from cache.
    }

    async fn insert_vista_value(&self, record: Record<CborValue>) -> Result<()> {
        let op = WriteOp::Insert(record);
        self.dio.write_queue.send(op).await?;
        Ok(())
    }
    // update, delete, replace similarly enqueue.
    // get_vista_value reads cache, falls through to on_query if registered.
}
```

### Capability re-derivation

The capabilities `DioShell` reports are computed from `master.capabilities()`
combined with `lens.callbacks`:

| Capability       | Source                                                          |
|------------------|-----------------------------------------------------------------|
| `can_insert`     | `master.can_insert() OR on_write is registered`                 |
| `can_update`     | `master.can_update() OR on_write is registered`                 |
| `can_delete`     | `master.can_delete() OR on_write is registered`                 |
| `can_subscribe`  | always `true` (Dio fans out events to Sceneries)                |
| `can_order`      | `cache.can_order()` — cache table source determines this        |
| `can_search`     | `cache.can_search()` — same                                     |
| `can_fetch_page` | `cache.can_fetch_page()` — same                                 |
| `can_fetch_next` | `cache.can_fetch_next() OR master.can_fetch_next()`             |

`can_order`/`can_search`/`can_fetch_page` reflect the cache because that's
what answers the queries. If the master can't sort but redb can (it can, on
indexed columns), the Dio reports `can_order = true`.

## Write queue and worker

The write worker is a single task per Dio. It owns the receiver end of the
mpsc queue and serializes writes:

```rust
async fn write_worker_loop(mut rx: mpsc::Receiver<WriteOp>, dio_inner: Arc<DioInner>) {
    let dio_handle = Dio { inner: dio_inner };
    while let Some(op) = rx.recv().await {
        if let Some(on_write) = &dio_handle.inner.lens.callbacks.on_write {
            let result = on_write(&dio_handle, op).await;
            if let Err(e) = result {
                // log; emit DioEvent::WriteFailed; do not panic
            }
        } else {
            // No on_write registered — default: write to cache and master.
            default_write(&dio_handle, op).await;
        }
    }
}
```

Backpressure: the queue has a fixed capacity (`LensDefaults::write_queue_capacity`,
default 256). Writes past the cap block the caller. This is intentional — it
surfaces overload rather than hiding it.

## Event bus

Each Dio owns a `tokio::sync::broadcast` channel that carries `DioEvent`
notifications. Sceneries subscribe; the Dio publishes.

```rust
pub enum DioEvent {
    RecordChanged { id: RecordId },
    RecordRemoved { id: RecordId },
    RecordInserted { id: RecordId },
    Invalidated,                      // wholesale: refresh just completed
    Refreshing,                       // refresh started
    WriteFailed { id: Option<RecordId>, error: String },
}
```

Sceneries hold a `broadcast::Receiver<DioEvent>` and react. The Lens itself
never directly touches Sceneries — all UI updates flow through the event bus.

The user's callbacks can publish into this bus via `dio.invalidate_record(id)`,
`dio.invalidate_all()`, `dio.patched(id, record)`. This is how `on_event`
turns external live-stream events into Scenery updates.

## Sceneries

Three trait shapes:

```rust
pub trait TableScenery: Send + Sync {
    // Cheap synchronous reads — must be hot-path safe.
    fn row_count(&self) -> usize;
    fn has_more(&self) -> bool;
    fn estimated_total(&self) -> Option<usize>;
    fn row(&self, idx: usize) -> Option<Arc<EnrichedRecord>>;

    // UI-driven hints.
    fn set_viewport(&self, range: Range<usize>);
    fn request_load_more(&self);
    fn request_refresh(&self);
    fn set_search(&self, query: Option<String>);
    fn set_sort(&self, column: Option<String>, dir: SortDir);

    // Notification.
    fn subscribe(&self) -> watch::Receiver<Generation>;
}

pub trait RecordScenery: Send + Sync {
    fn record(&self) -> Option<Arc<EnrichedRecord>>;
    fn status(&self) -> RecordStatus;

    fn request_refresh(&self);
    fn subscribe(&self) -> watch::Receiver<Generation>;
}

pub trait ValueScenery: Send + Sync {
    fn value(&self) -> Option<CborValue>;
    fn status(&self) -> ValueStatus;

    fn request_refresh(&self);
    fn subscribe(&self) -> watch::Receiver<Generation>;
}
```

`Generation` is a `u64` that increments on any change. UI adapters bridge the
`watch::Receiver<Generation>` into their native notification system.

### TableSceneryBuilder

```rust
pub struct TableSceneryBuilder {
    dio: Arc<DioInner>,
    conditions: Vec<Condition>,
    sort: Option<(String, SortDir)>,
    search: Option<String>,
    page_size: usize,                  // default 50
    eager: bool,                       // load all up front (small datasets)
}

impl TableSceneryBuilder {
    pub fn where_eq(mut self, col: impl Into<String>, value: impl Into<CborValue>) -> Self { … }
    pub fn sort(mut self, col: impl Into<String>, dir: SortDir) -> Self { … }
    pub fn search(mut self, q: impl Into<String>) -> Self { … }
    pub fn page_size(mut self, n: usize) -> Self { … }
    pub fn eager(mut self) -> Self { … }
    pub fn open(self) -> Arc<dyn TableScenery> { … }
}
```

### Scenery internal state

```rust
struct TableSceneryState {
    dio: Arc<DioInner>,

    // Query parameters (immutable after open, except search/sort which are mutable).
    conditions: Vec<Condition>,
    sort: RwLock<Option<(String, SortDir)>>,
    search: RwLock<Option<String>>,
    page_size: usize,

    // Loaded data.
    rows: RwLock<Vec<RowSlot>>,       // ordered, sparse — None where not yet loaded
    has_more: AtomicBool,
    estimated_total: RwLock<Option<usize>>,

    // UI hints.
    viewport: RwLock<Range<usize>>,

    // Notification.
    generation: AtomicU64,
    notify_tx: watch::Sender<u64>,

    // Background fetch task.
    fetcher: Mutex<Option<JoinHandle<()>>>,
}

enum RowSlot {
    Loaded(Arc<EnrichedRecord>),
    Pending,                          // fetch in flight
    Empty,                            // unknown yet
}
```

`set_viewport` triggers prefetch around the requested range. `request_load_more`
extends `has_more` frontier. The background fetcher consumes prefetch requests
and updates `rows`, bumps `generation`, notifies.

### EnrichedRecord

```rust
pub struct EnrichedRecord {
    pub record: Record<CborValue>,
    pub status: RowStatus,
    pub dirty_fields: Option<Vec<String>>,     // when wrapping an in-progress edit
    pub fetched_at: Option<Timestamp>,
}

pub enum RowStatus {
    Fresh,
    Stale,
    Loading,
    PendingWrite,
    WriteFailed { error: String },
    NotFound,
}
```

`EnrichedRecord` is what Sceneries hand to UI code. It preserves the underlying
`Record<CborValue>` and layers metadata on top. The `dirty_fields` slot
supports form-edit scenarios where only some columns have unsaved changes.

For non-Scenery contexts (CLI, business logic via `dio.vista()`), bare
`Record<CborValue>` flows through unchanged — the enrichment is Scenery-only.

## Hot tier

The hot tier is an in-memory `moka::future::Cache<RecordId, Arc<EnrichedRecord>>`
owned per Dio. It's populated on every cache read and on every Scenery row
load. TTL and size are inherited from `LensDefaults::cache_ttl`.

Purpose: keeps `TableScenery::row(idx)` synchronous and fast. The Scenery's
row vector holds `Arc<EnrichedRecord>` references that come from the hot tier;
when the tier evicts, the references survive until the Scenery drops them.

Hot-tier writes happen on the same task that updates `rows` and bumps
`generation`, so a single notification covers both the hot-tier population
and the Scenery state.

## Refresh scheduling

Per-Dio task spawned at `make_dio` time:

```rust
async fn refresh_loop(dio_inner: Arc<DioInner>, interval: Duration) {
    let mut tick = tokio::time::interval(interval);
    tick.tick().await;                             // skip the immediate fire
    let dio_handle = Dio { inner: dio_inner.clone() };
    loop {
        tick.tick().await;
        if let Some(cb) = &dio_inner.lens.callbacks.on_refresh {
            let _ = cb(&dio_handle).await;         // errors are logged, not propagated
        }
        dio_inner.event_bus.send(DioEvent::Invalidated).ok();
    }
}
```

Manual refresh via `dio.refresh().await` fires the same callback synchronously
and publishes `Invalidated` on completion.

## Cross-Dio interactions

Dios are independent. A change in one Dio doesn't propagate to another. If you
want cross-Dio invalidation (e.g., editing an `Order` invalidates a `Client`
view that aggregates orders), the user's `on_write` callback explicitly calls
`other_dio.invalidate_record(...)` or `other_dio.refresh()`.

Future direction: a Lens-level event bus that all Dios under the lens publish
into, with subscribers able to filter by Dio name. Not in v1.

## Concurrency model

- One write worker task per Dio, processing `WriteOp`s sequentially.
- One refresh task per Dio (if `refresh_every` is set).
- One background fetcher per Scenery, processing prefetch requests.
- The event bus uses `tokio::sync::broadcast`, lagging consumers see lost-event
  errors; Sceneries respond by re-reading state and bumping generation.

All shared state lives behind `Arc`. Mutable state uses `tokio::sync::RwLock`
or `parking_lot::Mutex` depending on whether the lock is held across awaits.
The hot tier uses `moka` which is async-aware.

## Error handling

Diorama errors fall into three categories:

1. **Setup errors** — invalid Lens configuration, cache backend unreachable.
   Surface as `Result<Lens, LensBuildError>` at `build()` time.
2. **Operation errors** — `dio.vista().insert(...)` may fail synchronously
   (queue full) or asynchronously (the queued write rejected by master). Sync
   errors return `Result`; async errors emit `DioEvent::WriteFailed`.
3. **Callback errors** — user callbacks return `Result<()>`. Errors are
   logged via `tracing` and emitted as `DioEvent::WriteFailed` or
   `DioEvent::RefreshFailed`. The Dio survives; callbacks fire again on the
   next trigger.

No callback failure ever poisons the Dio. The user's strategy decides whether
a failed refresh marks data stale or hides it; Diorama just reports.

## File layout

```
vantage-diorama/src/
├── lib.rs                    re-exports
├── lens/
│   ├── mod.rs                Lens, LensBuilder
│   ├── callbacks.rs          callback type aliases + boxing helpers
│   ├── defaults.rs           LensDefaults
│   └── build.rs              build() and validation
├── dio/
│   ├── mod.rs                Dio, DioInner
│   ├── shell.rs              DioShell : TableShell
│   ├── worker.rs             write queue worker
│   ├── refresh.rs            refresh task
│   ├── event_bus.rs          DioEvent + broadcast wiring
│   └── hot_tier.rs           moka wrapper
├── scenery/
│   ├── mod.rs                trait re-exports
│   ├── table.rs              TableScenery + TableSceneryBuilder + state
│   ├── record.rs             RecordScenery
│   ├── value.rs              ValueScenery
│   └── enriched_record.rs    EnrichedRecord + RowStatus
├── ops/
│   ├── write_op.rs           WriteOp enum
│   ├── query_descriptor.rs   QueryDescriptor (for on_query)
│   └── change_event.rs       ChangeEvent (for on_event)
└── error.rs                  LensBuildError, DioError
```

This layout mirrors `vantage-live`'s `live_table/` for the worker/event-consumer
split and follows the workspace convention of putting trait impls under
`impls/` subdirs (e.g., `dio/impls/table_shell.rs` for the `TableShell` impl
on `DioShell`).
