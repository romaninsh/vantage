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

The crate ships a self-contained CLI that exercises every feature
without needing an external server. A redb file plays the role of "the
remote database"; `LiveTable` wraps it.

```sh
# 1. Populate the master with sample data.
cargo run --example live_demo -- seed

# 2. Read everything. Run twice — first is a miss, second a hit
#    (you'll see ~80x speedup in the "wall time" line).
cargo run --example live_demo -- list
cargo run --example live_demo -- list

# 3. Insert through the LiveTable. Cache is invalidated; next read
#    repopulates from master.
cargo run --example live_demo -- add d Donut 5
cargo run --example live_demo -- list

# 4. Look up one row.
cargo run --example live_demo -- get a

# 5. Push a fake "remote change" event and watch the cache invalidate.
#    The demo prints first list (miss), second list (hit), then the
#    event lands and the next list misses again.
cargo run --example live_demo -- event-then-list

# 6. Targeted event for one id.
cargo run --example live_demo -- event-then-list --id a

# 7. Show the LiveTable's wiring.
cargo run --example live_demo -- info

# 8. Watch every cache hit/miss and queue event in tracing output.
cargo run --example live_demo -- --debug list
RUST_LOG=vantage_live=trace cargo run --example live_demo -- --debug add e Eclair 9
```

Useful flags:

- `--master <PATH>` — point at a different redb file (default
  `./demo-master.redb`).
- `--cache mem|none|<PATH>` — pick a cache backend.
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
(`MemCache`, `NoCache`, `RedbCache` is on the roadmap), pluggable
event source via `LiveStream`.

Out of scope for v1 — see `DESIGN.md`:

- Multi-page glue when UI ipp > master ipp.
- Per-page surgical invalidation.
- `RecordEdit` / snapshot-based dirty tracking.
- TTL-based expiry.
- The entity-shaped traits (`DataSet<E>` etc.) — `Record<Value>` only.
