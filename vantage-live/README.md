# vantage-live

A write-through cache layer that wraps any `AnyTable` (the "master") and
adds a local cache plus an optional event stream. Reads consult the cache
first; misses fall through to the master and populate the cache on the
way back. Writes are queued on a worker task and applied to the master,
then the cache is invalidated. An optional `LiveStream` keeps the cache
in sync with out-of-band changes (SurrealDB LIVE, Kafka, etc.).

The point: make UI code non-blocking when it shouldn't be. Scrolling a
list of clients on a phone shouldn't wait for the network on every page
change, and editing a record shouldn't lock the form while the write is
in flight.

For the architectural rationale see [`DESIGN.md`](./DESIGN.md).

## Demo

The crate ships a CLI with two master modes — `local` (redb file
pretending to be a remote database, full read/write/event cycle) and
`api` (JSONPlaceholder over the public internet, read-only but the
cache benefit is dramatic).

### Local mode — full cycle

```sh
# Populate the master.
cargo run --example live_demo -- local seed

# Read everything. Run twice — first is a miss, second a hit
# (~80x speedup in the "wall time" line).
cargo run --example live_demo -- local list
cargo run --example live_demo -- local list

# Insert through the LiveTable. Cache is invalidated; next read
# repopulates from master.
cargo run --example live_demo -- local add d Donut 5
cargo run --example live_demo -- local list

# Push a fake "remote change" event and watch the cache invalidate.
cargo run --example live_demo -- local event-then-list
cargo run --example live_demo -- local event-then-list --id a   # targeted

# Watch the dance in tracing output.
cargo run --example live_demo -- --debug local list
RUST_LOG=vantage_live=trace cargo run --example live_demo -- --debug local add e Eclair 9
```

### API mode — JSONPlaceholder

`https://jsonplaceholder.typicode.com` over the public internet. First
fetch hits the network (50–500ms), subsequent fetches are
microseconds-fast.

```sh
# Pick a resource. JSONPlaceholder offers users / posts / comments / albums / todos.
cargo run --example live_demo -- api users list
cargo run --example live_demo -- api posts list
cargo run --example live_demo -- api comments list

# Fetch by id.
cargo run --example live_demo -- api users get 1

# Pagination is pushed into the URL — each page caches under its own key.
cargo run --example live_demo -- api users list --page 1 --limit 3
cargo run --example live_demo -- api users list --page 2 --limit 3

# Filter with --filter field=value (eq-condition). The filter becomes
# part of the URL (?postId=1) and part of the cache_key, so different
# filters cache under different slots — postId=1 and postId=2 don't
# trample each other.
cargo run --example live_demo -- api comments list --filter postId=1 --limit 5
cargo run --example live_demo -- api todos    list --filter completed=true --limit 10
```

### Persistent disk cache

A folder path becomes a `RedbCache` — cache survives process restarts.
With API mode this gives a network-call → microseconds speedup *across
runs*, which is the closest the demo gets to a real "mobile UI cached
on disk" scenario.

```sh
cargo run --example live_demo -- --cache ./vlive-cache api users list   # cold network
cargo run --example live_demo -- --cache ./vlive-cache api users list   # warm disk
```

### Flags

- `--master <PATH>` — redb file for the `local` master (default
  `./demo-master.redb`).
- `--cache mem|none|<FOLDER>` — pick a cache backend. A folder path
  becomes a `RedbCache`, persisting cache state across process
  restarts.
- `--debug` — emit tracing spans (cache hit/miss, queue events,
  invalidations).

## Programmatic use

```rust
use std::sync::Arc;
use vantage_live::{LiveTable, cache::MemCache};
use vantage_table::any::AnyTable;

// Wrap any AnyTable as the master.
let master  = AnyTable::from_table(my_table);
let cache   = Arc::new(MemCache::new());

// `cache_key` is caller-owned. Use a different key for a different
// view (different conditions, ordering, etc.).
let live = LiveTable::new(master, "clients", cache);

// LiveTable implements TableLike, so it slots into AnyTable too —
// generic code (UI adapters, axum handlers, etc.) doesn't know it's
// talking to a cache.
let any = AnyTable::from_table_like(live);
```

`LiveTable` implements the standard value-set traits from
`vantage-dataset` (`ReadableValueSet`, `WritableValueSet`,
`ActiveRecordSet`), so any consumer that already speaks `Record<Value>`
keeps working.

## Status

v1 covers: read-side cache keyed by caller-supplied `cache_key` plus
page number, write-queue worker that doesn't block callers, sloppy
invalidation on every write or live event, pluggable cache backends
(`MemCache`, `NoCache`, `RedbCache`), pluggable event source via
`LiveStream`.

Out of scope for v1 — see `DESIGN.md`:

- Multi-page glue when UI ipp > master ipp.
- Per-page surgical invalidation.
- `RecordEdit` / snapshot-based dirty tracking.
- TTL-based expiry.
- The entity-shaped traits (`DataSet<E>` etc.) — `Record<Value>` only.
