# Diorama Architecture

This describes the inside of `vantage-diorama`: the types behind the public
surface, how they own each other, the tasks they run and the rules that keep
a view's rows honest. It is for maintainers and for authors of adapters and
loaders. The role READMEs ([README_lens.md](README_lens.md),
[README_ui.md](README_ui.md) and the rest) cover the public surface;
[README_rust_dev.md](README_rust_dev.md) covers the module map, features and
tests.

## Layers

```text
+--------------------------------------------------------------------+
|                            Application                             |
|  dio.cached(): Vista      sceneries: Table / Record / Value  Servo |
+--------------------------------------------------------------------+
                 |                        |
+--------------------------------------------------------------------+
|                                Dio                                 |
|  master (narrowed Vista)   residual   cache table   event bus      |
|  write queue + worker      shared-rows registry     open views     |
|  augment scheduler         query indexes            live follow    |
+--------------------------------------------------------------------+
                 |
+--------------------------------------------------------------------+
|                                Lens                                |
|  cache backend   loader   refresher   writer   tuning   runtime    |
|  registry of live Dios                debug tap                    |
+--------------------------------------------------------------------+
                 |
+--------------------------------------------------------------------+
|        redb file / memory          master Vistas (any driver)      |
+--------------------------------------------------------------------+
```

One Lens serves many Dios, and one cache backend backs them all: each Dio
claims one named table in it. A Dio produces facades, sceneries and servos,
which are cheap to open and cheap to drop.

## Lens

A Lens is built once with `Lens::builder()` and is immutable after
`build()`, which validates the parts and returns `Arc<Lens>`:

```rust
pub struct Lens {
    pub(crate) cache: Arc<dyn CacheBackend>,
    pub(crate) callbacks: LensCallbacks,   // loader, refresher, writer
    tuning: Tuning,
    pub(crate) runtime: Handle,
    pub(crate) debug: DebugTap,
    pub(crate) dios: DioRegistry,
}
```

The builder takes:

- `cache(Cache)`: `Cache::redb(path)`, `Cache::memory()` or
  `Cache::backend(arc)` for any `CacheBackend`.
- `loader(Loader)`: how a Dio's cache is filled. `Loader::eager(start)`
  copies the whole set once per Dio, awaited by the build unless
  `.blocking(false)`. `Loader::paged(chunk)` fetches the windows views ask
  for. `Loader::paged(..).or_eager(..)` pages a master that advertises
  `Window` and copies one that doesn't. No loader means the cache is all
  there is.
- `refresher(Refresher)`: `Refresher::new(f)`, with `.every(interval)` for a
  timer and `.max_age(ttl)` to refresh a cache found too old at build or at
  open.
- `writer(f)`: where flashes go instead of the master. A writer lifts the
  Dio's write capabilities to all three.
- `tuning(Tuning)`: `write_queue` (256), `viewport_debounce` (50 ms),
  `augment_workers` (1), `refresh_on_open` (true), `aggregate_debounce`
  (250 ms).
- `runtime(handle)`, and `debug_datasource(name)` with `diagnostics`.

### Callbacks

Every callback receives an owned `LoaderCx` and returns a `'static` future:

```rust
type CxFn    = Box<dyn Fn(LoaderCx) -> BoxFuture<()> + Send + Sync>;
type ChunkFn = Box<dyn Fn(LoaderCx, Range<usize>, VistaQuery, ChunkSink) -> BoxFuture<()> + Send + Sync>;
type WriteFn = Box<dyn Fn(LoaderCx, ChangeFlash) -> BoxFuture<FlashOutcome> + Send + Sync>;
```

`LoaderCx` is the Dio a callback serves: `master()`, `cache()`, `fetch(window,
query)` and `reconcile(snapshot)`. It takes a settle mark when it is handed
out, so a `reconcile` through it skips rows whose flash settled after the
callback began listing; `Dio::reconcile` can't know when a snapshot was taken
and assumes "now".

### Why an owned context instead of a borrowed `&Dio`?

A borrowed Dio makes the callback's future borrow from its argument, which
needs a higher-ranked closure type that is hard to name and harder to store.
An owned, cheap-to-clone context makes the future `'static`, so one boxed
closure serves every Dio of the Lens and the future can be held across awaits.
It also narrows what a callback can do: there is no way to reach the Dio's
sceneries or its write queue from a loader, and so no way to flash the Dio it
is writing for. The writer is refused at runtime too (`writing_for`).

### The registry of live Dios

`lens.dio(vista).build()` first looks in `Lens::dios`. A Dio with the same
cache identity, the same augmentations `Arc` and the same `live` setting that
some holder still keeps is returned as it is, after a `max_age` check. The
registry holds `Weak`s: once every holder drops, the next build makes a new
Dio.

### Cache backend and cache identity

`CacheBackend` opens named `CacheTable`s; a `CacheTable` is deliberately
narrow: id-keyed rows with a `CacheStatus` (`Complete` or `Incomplete`), bulk
writes, a count, and three meta values. It has no conditions, sorting or
search. Query planning lives on the Dio side.

A Dio's table name is its cache identity: the vista's name, or
`name?<query key>` when the master's applied query plus the residual narrows
or orders it. Two Dios share a table only when their queries are the same.
Control characters, `%` and a `?` in the name are percent-encoded, so no
narrowed table can pass for another vista's.

### What persists

The redb backend stores each row as a status tag byte followed by the CBOR
record, and each table's meta in a sibling table:

- the grand total a fetch last stated, so a reopened paged view sizes itself
  before its first fetch (`ViewCount::Estimated` until restated);
- when the table was last refreshed, for `Refresher::max_age` across
  restarts;
- when the table last became a whole copy of the set, so a view can serve the
  whole set from the cache.

Row positions are not persisted. A view re-derives them when it opens: an
eager view reseeds from the cache in its order, a paged view refetches its
first window, and a two-pass view re-lists its index, from the whole copy when
the cache holds one and from the master otherwise. The persisted
`CacheStatus` is what lets a two-pass view resume hydration across a restart
instead of fetching every row's detail again.

## Dio

```rust
pub struct Dio {
    pub(crate) inner: Arc<DioInner>,
}

pub(crate) struct DioInner {
    lens: Arc<Lens>,
    master: Vista,                 // narrowed by every clause it accepted
    residual: VistaQuery,          // clauses the master refused
    query: VistaQuery,             // master's query + residual; its key is the cache identity
    cache: Arc<dyn CacheTable>,
    write_queue: mpsc::Sender<QueuedFlash>,
    event_bus: broadcast::Sender<DioEvent>,
    pending_flashes: Arc<PendingFlashes>,
    shared_rows: Mutex<HashMap<String, Weak<SharedRows>>>,
    table_views: Mutex<Vec<Weak<TableView>>>,
    query_indexes: Mutex<HashMap<String, Arc<QueryIndex>>>,
    augmentations: Option<Arc<Vec<Augmentation>>>,
    augment_scheduler: Arc<AugmentScheduler>,
    detail_state: Mutex<HashMap<String, DetailState>>,
    seed_complete: AtomicBool,
    // … refresh and worker handles, live-follow cancel, census counters
}
```

`Dio` is `Clone` and cheap: every clone shares the inner state. It lives until
its last strong holder drops: `Dio` clones, a `Servo`, a facade from
`cached()`, or a queued flash still landing. Sceneries hold it weakly. A built
Dio never changes; `derive()` builds a narrowed one, and `get_ref` /
`get_ref_target` build Dios over related rows on the same Lens.

### Building

`DioBuilder::build` resolves the id column (a Vista without one fails), the
query and the cache identity, checks the registry, and otherwise spawns:

1. Open the cache table and build `DioInner`.
2. Spawn the write worker and, with `Refresher::every`, the refresh timer.
3. On an augmented Dio, start the augment worker pool.
4. With `live()`, subscribe to the master's change stream before any copy.
5. Run the eager start when the loader has one for this master: awaited, or
   detached with `seed_complete` false until it lands and announces `Seeded`.
   Otherwise request a refresh if the cache is older than `max_age`.
6. Start the live follow, applying changes made during the copy over it.
7. Register the Dio in the Lens's registry.

### Surface

```rust
impl Dio {
    // Views
    pub fn cached(&self) -> Vista;
    pub fn table_scenery(&self) -> TableSceneryBuilder;
    pub fn record_scenery(&self, id: impl Into<String>) -> RecordSceneryBuilder;
    pub fn value_scenery<A: Aggregation<Output = CborValue>>(&self, agg: A) -> ValueSceneryBuilder<A>;
    pub async fn aggregate<A: Aggregation<Output = DerivedRows>>(&self, agg: A) -> Result<Dio>;
    pub async fn servo(&self, id: impl Into<String>) -> Result<Servo>;
    pub fn servo_new(&self, strategy: IdStrategy) -> Servo;

    // Narrowing and relations
    pub fn derive(&self) -> DeriveBuilder;
    pub async fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Dio>;
    pub async fn get_ref_target(&self, relation: &str) -> Result<Dio>;

    // Data in and out
    pub async fn flash(&self, flash: ChangeFlash) -> Result<FlashOutcome>;
    pub async fn apply_change(&self, change: ChangeEvent) -> Result<()>;
    pub async fn reconcile(&self, snapshot: IndexMap<String, Record<CborValue>>) -> Result<()>;
    pub async fn refresh(&self) -> Result<()>;
    pub async fn fetch(&self, window: Range<usize>, query: &VistaQuery) -> Result<Fetched>;
    pub async fn import_values(&self, rows: IndexMap<String, Record<CborValue>>,
                               progress: impl FnMut(usize, usize) -> ControlFlow<()> + Send)
        -> Result<ImportOutcome>;

    // Introspection
    pub fn master(&self) -> &Vista;
    pub fn residual(&self) -> &VistaQuery;
    pub fn id_column(&self) -> &str;
    pub fn subscribe_events(&self) -> broadcast::Receiver<DioEvent>;
    pub fn write_capabilities(&self) -> WriteCapabilities;
    pub fn downgrade(&self) -> WeakDio;
    pub async fn diagnostics(&self) -> Result<DioDiagnostics>;   // diagnostics
    pub fn debug_tap(&self) -> DebugTap;                         // diagnostics
}
```

`apply_change` is the one door for a single upstream change: `Inserted` and
`Updated` upsert the cache when they carry the row and publish
`RecordInserted` / `RecordChanged`; `Deleted` removes it and publishes
`RecordRemoved`; `Invalidated` refreshes. `reconcile` is the door for a full
set: rows that differ are written, cached rows absent from the snapshot are
deleted, rows with a flash in flight are left alone, the refresh and the whole
copy are stamped, and one `DatasetChanged` is published.

### Why doesn't `Dio` hand out its cache table?

Because a write to the raw cache table raises no event and ignores a flash in
flight. Code holding it could make the cache disagree with every open view, or
overwrite a value the user just saved with a snapshot taken before the save.
Every write that views must see goes through `flash`, `apply_change` or
`reconcile`. A loader still writes the table directly through
`LoaderCx::cache()` during an eager start, because nothing is shown from it
yet and the start announces `Seeded` when it lands.

## The facade: `DioShell`

`dio.cached()` is a plain `Vista` whose `TableShell` is `DioShell`. Its schema
is the master's with every column marked orderable, and it advertises what it
answers itself:

| Capability | Why |
|---|---|
| `Insert`, `Update`, `Delete` | as `Dio::write_capabilities`: the master's, or all three under a Lens writer |
| `Subscribe` | the event bus backs `watch`, re-evaluated through the handle's narrowing |
| `Search`, `FilterOps`, `Order` | pushed into the master when it takes them, evaluated over the rows read when it doesn't |
| `Window` | windowed over the rows read, hydrating augmented gaps first |
| `ConfineWrites` | every write stays inside the handle's narrowing and the Dio's set |

Not advertised: `Import` (bulk import is `Dio::import_values`), `Traverse`
(`Dio::get_ref`), `Cursor` and `NativeConditions`.

A read plans once. A handle that doesn't narrow reads the cache, with any
order applied over it, so sorting never changes which rows a handle shows. An
augmented Dio always reads the cache, because augmented columns exist nowhere
else. Otherwise the master takes what it can: when it took a condition or the
search the read goes to the narrowed master, whose answer covers the whole
set, and refused clauses are evaluated over it. Every read applies the Dio's
residual on top.

## Writes

`Dio::flash` is the one write path, and the facade's writes use it:

1. **Confine.** The write must stay inside the Dio's set: the residual through
   the master, the whole query under a Lens writer (which bypasses the master's
   narrowing). Equality conditions are filled into a record; a record or a
   patch result outside the set is refused as `Conflict` before anything is
   staged.
2. **Stage.** A flash with an id is written to the cache, marked in flight in
   `PendingFlashes` (so a reconcile skips it), and announced as
   `WritePending`.
3. **Queue.** The flash, its pre-image and a strong `Arc<DioInner>` join the
   write queue. A full queue makes the caller wait for room, which surfaces
   overload instead of hiding it.
4. **Write and settle.** One worker per Dio takes one flash at a time, in
   order, through the writer or the master. A confirmed write publishes
   `RecordInserted`, `RecordChanged` or `RecordRemoved`; a failed one restores
   the pre-image and publishes `WriteReverted`. With several flashes on one id
   in flight, a rollback restores the last confirmed row. An id-less flash that
   fails publishes `WriteFailed`.

The queued `Arc<DioInner>` is the drain guarantee: dropping the last external
handle never discards an accepted write. A panicking writer is a failed
write; the worker carries on. A writer that flashes the Dio it is writing for
gets an error instead of a deadlock.

## Event bus

Each Dio owns a `broadcast` channel of 64 `DioEvent`s:

```rust
#[non_exhaustive]
pub enum DioEvent {
    RecordChanged { id: String },
    RecordInserted { id: String },
    RecordRemoved { id: String },
    DatasetChanged,                     // membership moved: re-derive
    Seeded,                             // the eager start's whole set is in the cache
    Refreshing,                         // leading edge of a refresh
    WriteFailed { id: Option<String>, error: String },
    WritePending { id: String, kind: FlashKind },
    WriteReverted { id: String, error: String, kind: FlashKind },
    ViewportChanged { range: Range<usize> },
    RangeLoaded { range: Range<usize> },
    LoadFailed { range: Range<usize>, error: String },
    RefreshFailed { error: String },    // a live follow's re-read failed
    RecordLoadFailed { id: String, error: String },
    Hydrating { pending: usize },       // a facade read is about to hydrate
}
```

`ChangeEvent` is the upstream shape (what a LIVE stream or a webhook
delivers); `DioEvent` is the internal fan-out shape views react to. A
subscriber that falls more than 64 events behind gets `Lagged`, and a view's
reactor answers that by re-deriving its whole set, since it can't know what it
missed. A view's own viewport events are emitted by its loader and ignored by
its reactor.

## Table views

A table view is two objects: the `TableView` its caller owns, and the
`SharedRows` it reads.

```rust
pub(crate) struct TableView {
    id: ViewId,
    dio_weak: Weak<DioInner>,
    base: Vec<Condition>,                       // builder filters, fixed
    filters: RwLock<Vec<Condition>>,            // runtime filters
    search: RwLock<Option<String>>,
    order: RwLock<Option<(String, SortDirection)>>,
    cap: RwLock<Option<Cap>>,                   // Limit, Tail or FlippedTail
    shared: RwLock<Arc<SharedRows>>,            // held strongly
    shown: RwLock<Option<Range<usize>>>,
    signal: Arc<ViewSignal>,                    // generation + view-stats channels
    ticket: Option<Arc<AugmentTicket>>,         // two-pass round-robin place
    repointing: Mutex<()>,
    // … page size, titles_only, demand, sugared, refusal, census tally
}

pub(crate) struct SharedRows {
    key: String,                                // shared_key of `query`
    query: VistaQuery,                          // fixed at creation
    rows: RwLock<RowMap>,                       // sparse idx -> row, with status counts
    id_to_idx: RwLock<HashMap<String, usize>>,
    idx_to_id: Mutex<Option<Vec<String>>>,      // splice's reverse index, rebuilt when stale
    total: RwLock<Option<usize>>,
    total_kind: RwLock<TotalKind>,              // Exact, Remembered or Horizon
    views: Mutex<HashMap<ViewId, ViewHook>>,    // attached views, their viewports
    viewport_tx: mpsc::UnboundedSender<ViewportRequest>,
    serving: RwLock<Serving>,                   // Windows, Cache or Failed
    index: RwLock<Option<Arc<QueryIndex>>>,     // two-pass only
    tasks: TaskGuard,                           // reactor, viewport loop, list pass
    // … settled, placed, paged, two_pass, titles_only, load bookkeeping, debug state
}
```

The split puts each piece of state where its owner is. The query, the rows,
the total and the tasks that keep them current belong to the query, so views
of one query share them. The clauses, the cap, the shown range and the change
channels belong to the caller, so a view that re-points keeps its subscribers,
and two views over one set of rows never see each other's view-only changes.
`SharedRows` fans each bump out to its attached views through their hooks,
which hold the signals weakly.

### The shared-rows registry

`DioInner::shared_rows` maps `shared_key(query, titles_only)` (the Dio's cache
table, the query's order-insensitive key, and the `titles_only` flag) to
`Weak<SharedRows>`. Page size and demand are view settings and stay out of the
key. Rows whose last view has left count as gone: the registry never hands
them back, and `try_attach` refuses them under the same `views` lock that
`detach` takes, so a view never joins rows whose work was aborted. A sugared
view's rows are never registered.

### Opening a view

`TableSceneryBuilder::open`:

1. Validates the cap (`limit` with `tail` fails, a `tail` without an order
   fails), takes the Dio's order when the view has none, and requests a
   refresh if the cache is older than `max_age`.
2. Composes the query and its key, and attaches to live rows under that key
   when there are some: no seed, no fetch.
3. Otherwise builds `SharedRows::new`, seeds it from the cache, starts its
   tasks (the reactor, the viewport loop, and on two-pass rows the coalesced
   reseed), schedules the on-open fetch, and registers it. Losing the
   registration race to a concurrent open joins the winner's rows; ours drop
   and abort.
4. Builds the `TableView`, adds it to the Dio's live-view list, starts the
   tail follower for a `tail`, and emits the census line.

The on-open fetch runs for a view serving windows when
`Tuning::refresh_on_open` is set. It is `force_load`, so it reaches the master
even over a warm cache: the cache is keyed by id and only the master knows the
query's order. Its priority is `Essential` over an empty cache and
`Background` over a warm one.

### Re-pointing and copy-on-write

A setter takes `repointing`, writes the view's own clause and calls `repoint`:

- The new query's key equals the old one: only the view's own clauses moved;
  bump its stats.
- Live rows exist under the new key: detach from the old rows and attach to
  those, re-asking the view's viewport (free when they hold it).
- Otherwise build `SharedRows::copied_from(old)`: the rows, their places, the
  total and the settled flag, sharing the old index on two-pass rows until the
  reload re-points it. Publish it (or join a concurrent winner), then reload:
  a windows view refetches its viewport in place, `Essential`; other rows
  reseed on their reactor.

A filter change clears the copied total. A tail turning flipped drops the copy,
because its rows are in the other order. The old rows are never written: other
views of the old query keep reading them, and when the re-pointing view was
their last, `detach` aborted their work, including a load in flight for the old
query, which therefore never lands in the copy.

### Lifecycle

`TableView::drop` detaches from its rows. `SharedRows::detach` removes the
hook and, when it was the last, calls `TaskGuard::abort_all` under the same
lock. Aborting at detach rather than at the rows' drop matters: a task in the
middle of a load holds an upgraded `Arc<SharedRows>`, so waiting for the drop
would let a closed grid keep pulling until the load finished.

### The reactor

`reload_loop` holds the rows weakly and reacts to the bus:

| Event | Single-pass rows | Two-pass rows |
|---|---|---|
| `RecordChanged`, `RecordInserted`, `RecordRemoved` | splice the one row when the rows hold their ordered set whole; otherwise refresh | update or splice the row in the index |
| `DatasetChanged`, `Lagged` | windows: refetch the viewport in place (`Background`); cache: reseed | re-list the index |
| `Seeded` | cache-served rows reseed; windows ignore it | re-list the index |
| `Refreshing` | cache-served rows reseed | ignored |
| `WritePending`, `WriteReverted` | stamp the row `PendingWrite` / `WriteFailed` | same |
| `RecordLoadFailed` | ignored | stamp the row `LoadFailed`, or re-derive a refined view |

A splice finds the row's new slot by binary search with the query's
comparator, reading neighbours through the reverse index `idx_to_id`. Every
other writer of `rows` or `id_to_idx` invalidates that index, and the next
splice rebuilds it, so it never places a row by stale neighbours.

### Viewport pipeline

`set_viewport(loaded, shown)` maps `loaded` through the cap, records it as
the view's viewport in its hook, and queues a `ViewportRequest { view, range,
force_load, priority }`; `shown` is the view's own and only feeds `ViewStats`.
`viewport_loop` (`scenery/table/loader.rs`) holds one pending request per view
and absorbs requests until the channel has been quiet for
`Tuning::viewport_debounce`. A merged request takes the newest range, and
keeps `force_load` and `Essential` if any absorbed request carried them: both
describe the wait, not the range.

`run_chunk_callback` (`loader/dispatch.rs`) emits `ViewportChanged`, then:

- on two-pass rows, runs the detail pass for the range under the asking
  view's ticket (nothing for `titles_only`), and stops;
- otherwise picks the effective window (`choose_window`, edge-anchored, with
  the horizon probe past an inferred end), and stops when it is cached and not
  forced, or when the rows don't serve windows;
- otherwise claims the in-flight range and calls the paged loader with a
  `ChunkSink` and the query its plan says the master takes whole.

`ChunkSink::push(idx, id, record)` binds the row into the visible map at once
and buffers it; a row with a flash in flight binds to the cached, staged value
and is not buffered. `ChunkSink::set_total` states the grand total.
`finish_chunk_load` (`loader/commit.rs`) writes the buffer to the cache in one
write, infers the total (stated, short page, horizon), re-sorts, settles the
rows, bumps once and publishes `RangeLoaded`, or on `Err` restores the bound
rows and publishes `LoadFailed`.

### Chunk loads: priority and cancellation

Every request carries a `vantage_core::Priority` set by its producer.
`Essential` means someone is waiting: `set_viewport`, `request_load_more`, the
reload after a re-point, the refill after a re-order dropped row positions,
and a cold on-open fetch. `Background` means nobody is: the refresh poll, a
warm on-open re-pull, hydration resuming after a two-pass index change, and
the horizon probe. `force_load` doesn't decide this; it means "consult the
master rather than the cache", which a search does as much as a poll. The
callback runs inside the viewport task under `priority.scope(..)`, so an HTTP
transport reads it to pick a retry policy; spawning the callback would lose
the task-local.

While a load is in flight, `viewport_loop` races it against the channel. A
request from the same view for a different range cancels it by dropping its
future, and `CancelOnDrop` restores exactly the slots the load bound,
putting back the row each one held. Slots the sink skipped (a client-sort
hold-back, an unchanged row) are not touched. A same-range request is
absorbed; a same-range `force_load` and any other view's request are parked
until the load finishes, and a superseded park ORs its `force_load` and
priority onto the request that wins.

That is why a load has two halves. `run_chunk_callback` owns the callback's
future and the cancel guard, and can be dropped. `finish_chunk_load` (the
commit, the total, the re-sort and the bump) runs to completion outside the
race: once the master has answered, nothing may unwind it.

### Two-pass loading

A Dio built with `augment(..)` makes every table view two-pass:

- A **list pass** lists cheap rows into a `QueryIndex`, the ordered ids of one
  variant (the builder filters and the order). The Dio keeps one index per
  variant and every view of it shares it, so re-opening a filter or order
  costs no list call. Runtime filters and search never key an index; a view
  refines them over it. The first list page pins the listing's source: the
  cache's whole copy when the Dio holds one, the master otherwise.
- A **detail pass** hydrates the ids in a view's `loaded` range. Every
  consumer (a view's viewport, a blocking facade read, a record scenery) queues
  ids through a ticket into the Dio's `AugmentScheduler`, whose worker pool
  (`Tuning::augment_workers`) drains tickets round-robin. An id in flight or
  already `Complete` is never fetched twice, and dropping a ticket withdraws
  what it still has queued. Each row's last answer lives in `detail_state`, so
  a row whose detail source has no record isn't re-asked on every tick.
- A **refined** view, one whose clauses name augmented values, is evaluated
  over the cache, and has to hydrate every listed row before it settles. A row
  that moves marks it dirty, and `reseed_loop` re-derives it once per batch,
  one debounce after the first mark.

A sweep runs inside the cancellable half of the viewport pipeline, so a
viewport that moves on drops it mid-flight; the superseding viewport sweeps
its own rows, and the dedup makes a half-finished sweep cost at most a repeat
of the rows it didn't reach.

### Serving

`Serving` records where a view's answer comes from, decided from its plan:

- `Windows`: a paged view whose master takes the whole query. Each window
  read from the master is the answer for its rows.
- `Cache`: every clause is evaluated over the complete set in the cache: an
  eager view, a two-pass view over its index, or a paged view whose Lens
  copies the whole set through `or_eager` when the master refused something.
- `Failed(LoadFailure)`: the master rejected a clause, or refused one with no
  complete set to evaluate it over. The view holds no rows until its query
  changes. It never shows a window as the filtered set.

### `EnrichedRecord`

```rust
#[non_exhaustive]
pub struct EnrichedRecord {
    pub record: Record<CborValue>,
    pub status: RowStatus,
    pub fetched_at: Option<SystemTime>,
}

#[non_exhaustive]
pub enum RowStatus {
    Fresh,
    Incomplete,                     // awaiting its detail pass
    PendingWrite,
    WriteFailed { error: String },
    LoadFailed { error: String },   // its detail fetch failed
}
```

Views hand these to UI code. The facade hands back bare `Record<CborValue>`;
statuses are a view concern.

## Record and value sceneries

A `RecordScenery` opens on the cache once (or on a seeded row), then reloads
its id on every event naming it. Its reads go through `read_admitted`, so a
row outside the residual is `NotFound`; on an augmented Dio it queues the row
on a ticket of its own and reads `Incomplete` until the detail lands. A
`load_on_miss` fetch caches only a row the residual admits, augmented in
memory first so the residual judges the whole row.

A `ValueScenery` and a derived aggregate Dio are both mounts of the aggregate
engine (`aggregate/engine.rs`): compute once so the mount opens holding a
value, then recompute on source events with a leading edge and one trailing
flush per `Tuning::aggregate_debounce`, publishing only a changed output. The
rows they reduce are the Dio's admitted rows, hydrated first when the residual
or the aggregation's `Reads` names an augmented value.

A `Servo` holds a strong `Dio`, a baseline and a draft. Its bus task absorbs
upstream changes into untouched fields; `flash()` sends the diff through
`Dio::flash`.

## Refresh

- `Dio::refresh` publishes `Refreshing`, runs the augmentation refresh pass
  on an augmented Dio, then the Lens's refresher, and publishes
  `DatasetChanged` (unless the refresher reconciled, which already did).
  Without either, it re-reads the set the way it was loaded: a paging Dio
  publishes `DatasetChanged` so each view refetches its window, an eager one
  re-runs its start, any other reconciles with `master.list()`.
- The timer (`Refresher::every`) sleeps first, then takes the scheduled path,
  which leaves rows known to have no detail record alone. Its errors are
  logged.
- `refresh_if_stale` runs at build and at every table or record open when the
  refresher has a `max_age`: at most one check per Dio at a time, never while a
  detached eager start is still copying.
- A live follow (`DioBuilder::live`) applies the master's change stream through
  `apply_change`. A lagged or failed stream re-reads the set, publishing
  `RefreshFailed` and retrying with backoff when that fails.

## Concurrency model

- One write worker per Dio, one flash at a time.
- At most one refresh timer per Dio, and one live-follow task.
- Per set of shared rows: one reactor, one viewport loop, and on two-pass rows
  one reseed loop and any detached list pass, all held weakly and aborted by
  the last view's detach.
- One augment worker pool per augmented Dio, started on first need and aborted
  when the Dio drops.
- One recomputation task per value scenery and per derived Dio.

Shared state lives behind `Arc`. Locks held across an await are tokio locks
(`reseed_lock`, the worker and refresh handles); everything else is a
`std::sync` lock taken and released without awaiting. Where two locks are
taken together the order is fixed: `rows` before `id_to_idx`, and
`idx_to_id` after both.

## Errors

- **Build.** `LensBuilder::build` validates the tuning, the loader and the
  refresher and fails with the setting named. `DioBuilder::build` fails for a
  Vista without an id column, a cache table that won't open, a blocking eager
  start that fails, or a live subscription that can't be made.
- **Writes.** `Dio::flash` returns the write's error after the rollback, and
  the views show `WriteFailed` for the row.
- **Loads.** A failed window publishes `LoadFailed` and leaves the visible map
  as it was. A view that can't answer its query reads `LoadState::Failed`
  with a `LoadFailure` naming the table and the clause.
- **Background work.** Timer refreshes, `max_age` refreshes and scenery
  `refresh_source` calls log their errors. None of them poisons the Dio; the
  next trigger runs again.

## Debug stream

Every Lens carries a `DebugTap`, off by default and cheap to check. With the
`diagnostics` feature, `LensBuilder::debug_datasource(name)` turns it on for
every Dio of that Lens. Each call site checks `tap.enabled()` before doing any
work a line needs, so an off tap costs a branch.

Lines go to `info` under the one target `vantage_diorama::debug`, shaped
`<datasource>  <tag>  <clause>`, with human units (`3.0s`, `24KB`,
`200,000`). The tag is the grep anchor, from a closed set: `dio`, `source`,
`scenery`, `census`, `viewport`, `cache`, `payload`, `total`, `sort`,
`search`, `filter`, `hydrate`, `derive`, `summary`. `fetch #N` and `list #N`
tie a request to its outcome; lines from one load are not emitted in a fixed
order, so correlate on the number. The table of tags and what each says lives
in the `debug` module's documentation (`src/debug.rs`).

The census line fires on every table-view, record-scenery and servo open and
close, with the live counts on that Dio and the process's peak RSS.
`stats::debug_summary_lines()` renders the session's fetch ledger, the live
counts and the process cost; `stats::emit_debug_summary()` logs them
unconditionally, for a shutdown hook to call.

## File layout

```text
vantage-diorama/src/
├── lib.rs              root re-exports and the prelude
├── lens.rs             Lens; lens/: builder, cache, cache_backend, callbacks,
│                       chunk_sink, loader, loader_cx, memory_cache, redb_cache,
│                       refresher, registry, spawn, tuning
├── dio.rs              Dio, WeakDio, Generation; dio/: builder, derive, inner
│                       (shared_rows, sceneries), shell, flash, worker, pending,
│                       ingress, refresh, watch, fetch, import, plan, residual,
│                       open, diagnostics, augment_passes, augment_scheduler,
│                       query_index, detail_state, whole_copy, event_bus
├── scenery.rs          scenery/: table, record, value, sugar, enriched_record
├── scenery/table.rs    TableScenery, LoadState; table/: builder, view, state,
│                       loader, reactor, plan, two_pass, cap, count, row_map,
│                       view_stats, handle, load_failure
├── aggregate.rs        aggregate/: aggregation, builtin, catalog, coerce, cmp,
│                       engine, mount, source, value, derived
├── augment.rs          Augmentation, Detail, Source, Fetch, MergeRule
├── servo.rs            Servo; servo/: draft, flash, delete, tracking
├── ops.rs              ops/: change_event, change_flash, flash_outcome,
│                       flash_rejection
├── debug.rs            DebugTap, tapline!, formatters; debug/process.rs
├── stats.rs            live counts; stats/: ledger, summary
└── rhai.rs             Servo as a Rhai type
```

A module `x.rs` keeps its children in `x/`; there is no `mod.rs` in `src/`.

## Invariants

- A view's setters never write its `SharedRows`; they move the view.
- Rows whose last view has left are never handed out again, and their work is
  aborted at that detach.
- Every writer of `rows` or `id_to_idx` invalidates `idx_to_id`.
- A cancelled load restores exactly the slots it bound.
- A commit, once the master has answered, runs to completion.
- No reconcile-shaped write overwrites a row with a flash in flight.
- A view never shows a window as a filtered set.
