# vantage-live design

## What this is

A `LiveTable` wraps an existing `AnyTable` (the "master") and adds a local
cache. Reads consult the cache first; misses fall through to the master and
populate the cache on the way back. Writes go to the master and invalidate
the cache. Optionally, an external event source (SurrealDB LIVE, a Kafka
topic, anything that can produce `LiveEvent`s) keeps the cache fresh
without polling.

The point is to make UI code non-blocking when it shouldn't be — scrolling
through a list of clients on a phone shouldn't wait for the network on
every page change, and editing a record shouldn't lock the form while the
write is in flight.

```rust
// A regular table, somewhere remote
let clients_remote = Client::surreal_table(db);

// Wrap it. "clients" is the cache key — caller chooses, caller owns.
let clients = LiveTable::new(
    AnyTable::from_table(clients_remote),
    "clients",
    RedbCache::open("./cache.redb")?,
);

// LiveTable implements TableLike, so it slots into AnyTable too —
// UI code doesn't know it's talking to a cache.
let any = AnyTable::new(clients);
```

`LiveTable` implements `TableLike`, so anywhere your code accepts an
`AnyTable` it accepts a `LiveTable`. The wrapping is invisible to
consumers.

## Scope

In v1:

- Implements only the value-set traits (`ReadableValueSet`,
  `WritableValueSet`, `InsertableValueSet`, `ActiveRecordSet`). Cache
  stores `Record<ciborium::Value>` end-to-end — same shape `AnyTable`
  uses.
- Read-side cache, keyed by caller-supplied `cache_key` plus page number.
- Writes routed to master (or a caller-supplied alternative target),
  queued on a worker task so the call site doesn't block.
- Sloppy invalidation: any write or live event blows the entire cache
  for that `cache_key`.
- Pluggable cache backend (`RedbCache`, `MemCache`, `NoCache`).
- Pluggable event source via the `LiveStream` trait.

Not in v1:

- The entity-shaped traits (`DataSet<E>` / `ReadableDataSet<E>` / etc.).
  Callers who want typed reads deserialise the cached `Record<Value>`
  themselves; we add the entity layer if a real workload demands it.
- Multi-page glue (when UI ipp > master ipp). The field is stored on
  the LiveTable but ignored; we'll wire it in once a real workload
  needs it.
- Per-page surgical invalidation. Sloppy is good enough until proven
  otherwise.
- `RecordEdit` / snapshot-based dirty tracking. Different concern,
  different crate, comes later.
- TTL-based expiry.

## Architecture

### The contract

`LiveTable` implements the standard *value-set* traits from
`vantage-dataset`: `ReadableValueSet`, `WritableValueSet`,
`InsertableValueSet`, and `ActiveRecordSet` (auto-derived from the
previous two). It also implements `TableLike`, so wrapping into
`AnyTable` works without an adapter. **No new public dataset traits.** A
consumer that already speaks `Record<Value>` doesn't need to learn
anything new.

Out of scope for v1: the entity-shaped traits (`DataSet<E>`,
`ReadableDataSet<E>`, `WritableDataSet<E>`, `ActiveEntitySet<E>`).
`AnyTable` is a `Record<Value>`-shaped abstraction anyway, so the cache
operates on records throughout. Entity-level wrapping can come later if
we find a real use case — until then, it's a layer that consumers can
build on top by deserialising records themselves.

Everything below is what the trait impls do internally — a cache lookup
in front of every read, a queue + worker behind every write, a
`LiveStream` keeping the cache honest.

### The struct

```rust
pub struct LiveTable {
    master: Arc<RwLock<AnyTable>>,
    cache_key: String,
    cache: Arc<dyn Cache>,
    custom_write_target: Option<Arc<RwLock<AnyTable>>>,
    write_queue: mpsc::Sender<WriteOp>,         // internal, not pub
    live_stream: Option<Arc<dyn LiveStream>>,

    // Master ipp is set once at construction. Changing it would
    // invalidate every cached page anyway — make a new LiveTable.
    master_ipp: Option<i64>,

    // Pagination state from set_pagination(); used to compute
    // the cache page suffix on each read.
    pagination: RwLock<Option<Pagination>>,
}
```

- `master` is `Arc<RwLock<…>>` so swap doesn't break outstanding handles.
- `master_ipp` is immutable after construction; we store it but v1 doesn't
  use it (we trust caller to keep UI ipp ≤ master ipp; multi-page glue
  comes later).
- `custom_write_target` is `None` by default — writes go to master.
  Override when you want writes to land somewhere other than where reads
  came from (e.g. a "submissions" table that gets reviewed before merging
  into the main one).

### Cache (infrastructure trait, not a dataset trait)

The dataset traits don't have a notion of "cache slot," so this is one
piece of new surface — but it's an internal building block, not a public
contract for consumers.

```rust
#[async_trait]
pub trait Cache: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<CachedRows>>;
    async fn put(&self, key: &str, rows: CachedRows) -> Result<()>;

    /// Drop everything under a prefix. v1 invalidation calls this with
    /// the bare `cache_key` — every page suffix below it goes.
    async fn invalidate_prefix(&self, prefix: &str) -> Result<()>;
}

pub struct CachedRows {
    pub rows: IndexMap<String, Record<ciborium::Value>>,
    pub fetched_at: SystemTime,
}
```

Three impls in v1:

- `RedbCache` — disk-backed, takes a folder. Inside, one redb file
  (`vlive.redb`) with one redb table per `cache_key`, namespaced
  `__vlive__{cache_key}`. Sub-keys (`page_n`, `id/foo`) are `&str`
  inside that table; values are CBOR-encoded `CachedRows`.
  `invalidate_prefix(cache_key)` drops the whole redb table — O(1)-ish.
  redb's exclusive file lock means one process per cache folder.
- `MemCache` — `Arc<RwLock<HashMap<String, CachedRows>>>`. Fast, fine for
  tests and short-lived processes.
- `NoCache` — every method is a no-op / returns `None`. Equivalent to
  bypassing the LiveTable wrapper, useful for parity tests.

### Read path — what `list_values` actually does

```rust
impl ReadableValueSet for LiveTable {
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Record<Self::Value>>> {
        let page = self.pagination.read().clone().unwrap_or_default();
        let key  = format!("{}/page_{}", self.cache_key, page.get_page());

        if let Some(cached) = self.cache.get(&key).await? {
            return Ok(cached.rows);
        }
        let mut master = self.master.write().await;
        master.set_pagination(Some(page));
        let rows = master.list_values().await?;
        self.cache
            .put(&key, CachedRows { rows: rows.clone(), fetched_at: now() })
            .await?;
        Ok(rows)
    }

    async fn get_value(&self, id: &Self::Id) -> Result<Option<Record<Self::Value>>> {
        // Single-row reads skip the page math:
        let key = format!("{}/id/{}", self.cache_key, id);
        // … same shape as list_values, but cache.get/put on the per-id key …
    }
}
```

### Write path — `WriteOp` is private

```rust
// internal — not in lib.rs's public surface
enum WriteOp {
    Insert  { id: Id, record: Record<Value>, reply: oneshot::Sender<Result<Record<Value>>> },
    Replace { id: Id, record: Record<Value>, reply: oneshot::Sender<Result<Record<Value>>> },
    Patch   { id: Id, partial: Record<Value>, reply: oneshot::Sender<Result<Record<Value>>> },
    Delete  { id: Id,                          reply: oneshot::Sender<Result<()>> },
    DeleteAll {                                reply: oneshot::Sender<Result<()>> },
    InsertReturnId { record: Record<Value>,    reply: oneshot::Sender<Result<Id>> },
}

impl WritableValueSet for LiveTable {
    async fn insert_value(&self, id: &Self::Id, record: &Record<Self::Value>)
        -> Result<Record<Self::Value>>
    {
        let (tx, rx) = oneshot::channel();
        self.write_queue
            .send(WriteOp::Insert { id: id.clone(), record: record.clone(), reply: tx })
            .await?;
        rx.await?
    }
    // replace_value / patch_value / delete / delete_all → same pattern
}

impl InsertableValueSet for LiveTable {
    async fn insert_return_id_value(&self, record: &Record<Self::Value>) -> Result<Self::Id> {
        // same queue dispatch, awaits the InsertReturnId oneshot
    }
}
```

The worker task drains the queue, applies each op against
`custom_write_target.unwrap_or(master)`, and on success calls
`cache.invalidate_prefix(&cache_key)`. Failure modes:

- Master rejects the write → the `oneshot` carries the `Err` back to the
  caller. Cache stays untouched (no false invalidation).
- Worker panics → all pending `oneshot`s drop, callers get `RecvError`.
  Worker is supervised; a new one starts on next `LiveTable::new`.

Fire-and-forget callers wrap `insert_value(...).await` in `tokio::spawn`
and ignore the future — same pattern that works on any `WritableValueSet`.

### LiveStream trait

```rust
#[async_trait]
pub trait LiveStream: Send + Sync {
    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = LiveEvent> + Send>>;
}

pub enum LiveEvent {
    Changed,                                // generic "something moved"
    Inserted { id: String },
    Updated  { id: String },
    Deleted  { id: String },
}
```

V1 treats every event the same: invalidate the whole `cache_key`. The id
variants exist for future surgical invalidation; the generic `Changed`
covers stream sources that don't deliver row-level detail.

Implementations live in their own crates / modules:

- `vantage-surrealdb` provides `SurrealLiveStream` over LIVE queries.
- A future `vantage-kafka` or app code can ship a `KafkaLiveStream`.
- Tests use a `manual` stream that lets the test push events in.

### TableLike + AnyTable

`LiveTable` implements `TableLike`, so:

```rust
let live = LiveTable::new(any_master, "clients", cache);
let any  = AnyTable::new(live);    // drop-in for UI / generic code
```

`TableLike` metadata methods (`table_name`, `columns`, `id_field`,
`references`) pass through to master under a read lock. `set_pagination`
stores into LiveTable's own `pagination` field — that's how the cache
key gets the right page suffix.

## Testing strategy

### The fixture

Master is a real SurrealDB seeded with the bakery dataset (the same
`scripts/start.sh` + `scripts/ingress.sh` pattern that `vantage-surrealdb`
already uses). Cache is `MemCache` for fast inner-loop tests, plus a
smaller `RedbCache` suite for the "does it survive a process restart"
case. Both are dev-dependencies — neither lives in the runtime crate.

```toml
[dev-dependencies]
vantage-redb       = { path = "../vantage-redb" }
vantage-surrealdb  = { path = "../vantage-surrealdb" }
bakery_model3      = { path = "../bakery_model3" }
tempfile           = "3"
testcontainers     = "..."   # optional — see below
tokio              = { version = "1", features = ["full", "test-util"] }
```

Two ways to get SurrealDB up:

- **Manual**: assume `scripts/start.sh` ran, point at `localhost:8000`.
  Tests skip cleanly if no server is reachable. Same convention as
  `vantage-mongodb`'s `MONGODB_URL`. Fast feedback during development.
- **testcontainers**: a `surrealdb-test-helper` module spins up a
  fresh container per test class, runs ingress, hands back a `SurrealDB`
  handle. Slow but hermetic — used in CI.

The helper picks based on `LIVE_TEST_MODE=container|local`, defaulting
to `local`. CI sets `container`.

### Test layout

Step-numbered like vantage-mongodb / vantage-redb:

```
tests/
├── 1_cache_trait.rs       MemCache + RedbCache contract — get/put/invalidate_prefix
├── 1_live_event.rs        LiveEvent matchers, manual stream wiring
├── 2_live_table_read.rs   miss → master fetch → cache populated → hit
├── 2_pagination.rs        different pages stored under different keys; ipp immutability
├── 3_live_table_write.rs  insert/replace/patch/delete → master + cache invalidated
├── 3_custom_write_target.rs   writes route to alternate table, reads stay on master
├── 3_queue_concurrency.rs   N concurrent writers, ordering, oneshot reply correctness
├── 4_live_stream.rs       manual stream pushes; cache invalidated on each event
└── 5_anytable_wrap.rs     LiveTable wrapped via AnyTable, used through TableLike
```

The `1_*` files are pure-unit (no SurrealDB). `2_*` and up bring up
the real server.

### What each test category proves

- **Cache trait**: round-trip, TTL-shape data, prefix invalidation
  matches the trait contract on every backend.
- **Read**: cache miss path populates; second read same key is a hit
  (verified by stopping master mid-test and checking reads still
  succeed). Single-row `get_value` keys differently from list pages.
- **Pagination**: `set_pagination(page=1)` and `set_pagination(page=2)`
  produce distinct cache entries; constructing a LiveTable with
  `master_ipp` doesn't change behaviour in v1 but the value is
  retrievable.
- **Write**: write hits master, cache for `cache_key` is empty after,
  next read repopulates from master. `custom_write_target` routes
  writes elsewhere — reads on the LiveTable still see master.
- **Queue concurrency**: 100 concurrent `insert_value` calls all get
  their own `oneshot::Sender` reply, no cross-talk. FIFO ordering on
  the master.
- **Live stream**: `ManualLiveStream::push(LiveEvent::Updated{...})`
  triggers cache invalidation; next read re-fetches from master.
  Tested with both `MemCache` and `RedbCache`.
- **AnyTable**: same scenarios but go through `AnyTable::new(live)`,
  proving the wrapper is invisible.

### Helpers in `tests/common/`

```
tests/common/
├── mod.rs          re-exports
├── surreal.rs      bring up master, run ingress, hand back AnyTable
├── manual_stream.rs    ManualLiveStream — pushes events on demand
└── fixtures.rs     seed test data, shared assertions
```

Same pattern as vantage-mongodb's `tests/common/`.

## Observability

Multi-layer code is hard to debug without spans. A single
`live.list_values()` call walks through: pagination state, cache key
build, cache backend lookup, possibly a master read, possibly a cache
write. When something goes wrong — stale cache, double-invalidation,
queue stuck, missed live event — staring at error messages won't tell
you which layer dropped the ball.

`tracing` (the crate) gives us spans + structured logs without baking in
a logger. Apps wire up whatever subscriber they want; we just emit.

### Dependencies

```toml
[dependencies]
tracing = "0.1"
```

`tracing-subscriber` only as dev-dep / in the CLI example, not in the
library. Tests set up a subscriber once via `tracing_subscriber::fmt::try_init()`
inside a `ctor`-style helper.

### Span boundaries

```rust
#[tracing::instrument(skip(self), fields(cache_key = %self.cache_key, page))]
async fn list_values(&self) -> Result<...> { ... }

#[tracing::instrument(skip(self, record), fields(cache_key = %self.cache_key, id = %id))]
async fn insert_value(&self, id: &Self::Id, record: &Record<Self::Value>) -> Result<...> { ... }

#[tracing::instrument(skip_all, fields(cache_key = %self.cache_key))]
async fn worker_loop(...) { ... }

#[tracing::instrument(skip_all, fields(event_kind))]
async fn handle_live_event(&self, event: LiveEvent) { ... }
```

Five span-worthy boundaries:

- **Read path** (`list_values`, `get_value`, `get_some_value`)
- **Write path** entry point (the `insert_value` etc. methods that
  enqueue and await)
- **Worker loop** (drains queue, applies to master, invalidates cache)
- **Live stream loop** (consumes events, invalidates cache)
- **Cache backend operations** (one span per `get`/`put`/
  `invalidate_prefix` so RedbCache slowness is visible)

### Event levels

- `error!` — only when something went wrong that the caller will see as
  an `Err`. No silent error swallowing.
- `warn!` — recovery cases: cache backend errored on `put` (we still
  served the master result), worker restarted after panic.
- `info!` — lifecycle: LiveTable constructed, master swapped, live
  stream connected/disconnected.
- `debug!` — every cache hit/miss, every queue enqueue/drain, every
  live event invalidating.
- `trace!` — full record contents on writes, full row counts on reads.
  Off by default; `RUST_LOG=vantage_live=trace` flips it on.

### Structured fields

Every log line carries enough context to grep:

- `cache_key` — which LiveTable.
- `page` — which page on read paths.
- `id` — which row on write paths.
- `op` — `insert | replace | patch | delete | delete_all | insert_return_id`.
- `outcome` — `hit | miss | populated | invalidated | failed`.

Avoid logging the master's connection string, full record bodies (use
`trace!` for those), or anything that could leak credentials.

### Test integration

```rust
// tests/common/mod.rs
pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
}
```

Each test calls `common::init_tracing()` once. `cargo test` shows
nothing by default; `RUST_LOG=vantage_live=debug cargo test -- --nocapture`
shows the cache/queue dance.

## CLI example

`bakery_model4/examples/cli4.rs` is the reference: `db <entity>
<commands>` with `field=value` filters, `list / get / add / delete /
ref` verbs, YAML-driven entity registry. We mirror it almost exactly,
adding one thing — the cache layer is wired in between the entity
constructor and the command handler. Same UX, with cache-hit/miss
visible via `--debug`.

```
examples/
└── live_cli.rs
```

Invocation, identical surface to cli4 plus a couple of cache-related
flags:

```
db <entity> <commands>...

Flags:
  --debug              Show traced cache/queue activity
  --cache <path>       Use a redb file (default: in-memory)
  --no-cache           Bypass cache entirely (parity check vs cli4)
```

Sketch of the wiring (the only part that differs from cli4):

```rust
async fn run() -> Result<()> {
    let config = VantageConfig::from_file("bakery_model4/config.yaml")?;
    let db     = connect_surrealdb_with_debug(matches.get_flag("debug")).await?;

    if let Some(entity_name) = matches.get_one::<String>("entity") {
        // Build the master table the same way cli4 does
        let master = get_table(&config, entity_name, db)?;
        let any_master = AnyTable::from_table(master);

        // Pick a cache backend from flags
        let cache: Arc<dyn Cache> = match (matches.get_one::<String>("cache"),
                                           matches.get_flag("no-cache")) {
            (_, true)            => Arc::new(NoCache),
            (Some(path), false)  => Arc::new(RedbCache::open(path)?),
            (None, false)        => Arc::new(MemCache::default()),
        };

        // Wrap. Cache key matches the entity name — easy mental model.
        let live = LiveTable::new(any_master, entity_name, cache);

        // Wrap into AnyTable so handle_commands can be exactly cli4's body
        let any_live = AnyTable::new(live);

        handle_commands(any_live, commands).await?;
    }
    Ok(())
}
```

`handle_commands` is copied verbatim from cli4 but typed against
`AnyTable` instead of `Table<SurrealDB, EmptyEntity>`. Since LiveTable
implements `TableLike`, every cli4 verb (`list`, `get`, `add`,
`delete`, `field=value`, `ref`) keeps working without any awareness of
the cache.

### What the example demonstrates

- Plain reads: `db client list` runs once, then a second time and
  shows a cache hit (visible only with `--debug`).
- Conditioned reads: `db client name=Marty list` — caller is
  responsible for picking a different `cache_key` if they want this
  cached separately. v1 doesn't, so this hits the master each time.
  (Documented gotcha; the example surfaces it.)
- Pagination: `db product page=2 list` — different cache slot.
- Writes: `db bakery add "foo" '{"name":"X"}'` invalidates the
  bakery cache; next `list` re-fetches.
- `--cache ./data.redb`: same flow, but cache survives between
  invocations. Run twice in a row, second one is hot from disk.

The example doubles as a manual test harness for the cache layer
during development.
