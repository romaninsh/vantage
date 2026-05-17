# Configuring a Lens

A Lens is the long-lived apparatus that holds your caching strategy. You write
one per application — sometimes more if you have genuinely different policies
for different domains — and from it you make Dios cheaply.

This document walks through how to configure a Lens, and then six real-life
scenarios that exercise different cache strategies. UI is only one of them.
Vantage runs in API servers, on the edge, on mobile, inside data pipelines —
each context wants different behavior, and the Lens is where you express it.

## Anatomy of a Lens

A Lens owns four kinds of things:

1. **A cache backend.** Where the cached data lives. Default is redb on disk;
   you can substitute any `TableSource`-implementing crate.
2. **Lifecycle callbacks.** Functions you write that describe how data moves.
   `on_start` runs once when a Dio is made; `on_refresh` runs on a timer or on
   demand; `on_write` routes mutations; `on_event` handles external change
   notifications; `on_query` fills the cache lazily.
3. **Default policies.** TTLs, refresh intervals, write queue capacity,
   whether `on_start` blocks. These apply to every Dio under the lens.
4. **A runtime handle.** Tokio handle for spawning the per-Dio worker tasks.
   Picked up from the current runtime by default.

```rust
use std::sync::Arc;
use std::time::Duration;
use vantage_diorama::Lens;
use vantage_redb::Redb;

let cache_db = Arc::new(Redb::open("./local.redb").await?);

let lens = Lens::new()
    .cache_source(cache_db)
    .on_start(|dio| async move {
        let data = dio.master().list_values().await?;
        dio.cache().insert_values(data).await?;
        Ok(())
    })
    .refresh_every(Duration::from_secs(3600))
    .build()
    .await?;
```

After `.build()`, the Lens is immutable. You call `.make_dio(vista)` as many
times as you like; every Dio inherits the same callbacks and policies.

### Why does `on_start` block by default?

If you have a UI grid that mounts as the app launches and the user immediately
sees an empty table because `on_start` hasn't finished filling the cache yet,
that's a worse experience than a brief startup spinner. Blocking until
`on_start` returns means the first `dio.vista().list_values()` call sees data.

Turn it off when you'd rather show "loading…" UI than freeze startup:

```rust
.with_default(LensDefaults { on_start_blocking: false, ..Default::default() })
```

### Why are callbacks borrowing, not `'static`?

Borrowing lets a callback read from `&Dio` without cloning the Dio's internal
state. The closure itself is `'static` (no captured references with lifetimes);
the returned future borrows `&Dio` to call `.master()`, `.cache()`, etc. This
is the HRTB pattern and it's why callbacks look natural:

```rust
.on_refresh(|dio| async move {
    let fresh = dio.master().list_values().await?;
    dio.cache().replace_all(fresh).await?;
    Ok(())
})
```

If you need to capture extra state (a write-ahead-log handle, a metrics
client), use `move` and `Arc`:

```rust
let metrics = Arc::new(Metrics::new());
let metrics_for_cb = metrics.clone();

.on_write(move |dio, op| {
    let metrics = metrics_for_cb.clone();
    async move {
        let start = Instant::now();
        dio.master().apply(&op).await?;
        metrics.record_latency(start.elapsed());
        Ok(())
    }
})
```

## Scenario 1 — Desktop UI, slow backend, instant reads

A desktop admin app reads from a remote GraphQL API. Every screen mounts a
grid; users scroll, filter, sort. The API is fine for fetching data but slow
enough that hitting it on every interaction would be unusable.

**Strategy.** Eager load on Dio creation. Periodic background refresh. Reads
always served from the local cache. Writes go through both — cache first for
instant UI update, then API for persistence.

```rust
let lens = Lens::new()
    .cache_at("./admin-cache.redb")
    .on_start(|dio| async move {
        let data = dio.master().list_values().await?;
        dio.cache().insert_values(data).await?;
        Ok(())
    })
    .on_refresh(|dio| async move {
        let fresh = dio.master().list_values().await?;
        dio.cache().replace_all(fresh).await?;
        Ok(())
    })
    .on_write(|dio, op| async move {
        dio.cache().apply(&op).await?;         // instant UI feedback
        dio.master().apply(&op).await?;        // persist
        Ok(())
    })
    .refresh_every(Duration::from_secs(300))   // every 5 minutes
    .build()
    .await?;
```

The cache survives app restarts. If the API is unreachable on the next launch,
the cached data is still there — `on_start` will fail, but the cache continues
to serve reads until the next successful refresh.

### What if a refresh fails mid-day?

The refresh task logs the error and emits `DioEvent::Invalidated` (or not,
depending on configuration). The cache keeps serving the previous data. Your
UI can subscribe to a Scenery and surface a "last synced N minutes ago"
indicator from the watch generation.

## Scenario 2 — Mobile app, offline-first

A field worker uses the app from a van. Network comes and goes. Edits must
succeed offline and sync when the device returns to coverage. The user must
never see "save failed" because of bad signal.

**Strategy.** Lazy fetch on access. Local cache is the source of truth for the
session. Writes always succeed locally; an outbox queue holds them until the
network is back.

```rust
let outbox = Arc::new(OutboxQueue::open("./outbox.redb").await?);
let outbox_for_write = outbox.clone();
let outbox_for_refresh = outbox.clone();

let lens = Lens::new()
    .cache_at("./mobile-cache.redb")
    .on_query(|dio, query| async move {
        // First time this query is seen, fetch from master if online.
        if is_online() {
            let rows = dio.master().with_query(query.clone()).list_values().await?;
            dio.cache().insert_values(rows).await?;
        }
        Ok(())
    })
    .on_write(move |dio, op| {
        let outbox = outbox_for_write.clone();
        async move {
            dio.cache().apply(&op).await?;             // always succeeds
            outbox.enqueue(dio.name(), op).await?;     // sync later
            Ok(())
        }
    })
    .on_refresh(move |dio| {
        let outbox = outbox_for_refresh.clone();
        async move {
            if !is_online() { return Ok(()); }
            outbox.drain_for(dio.name(), |op| dio.master().apply(op)).await?;
            let fresh = dio.master().list_values().await?;
            dio.cache().replace_all(fresh).await?;
            Ok(())
        }
    })
    .refresh_on_network_resume()
    .build()
    .await?;
```

The `OutboxQueue` is user code — Diorama doesn't ship one because the right
shape (per-record vs per-op, idempotency keys, conflict resolution) depends on
your domain.

### Won't the cache get out of sync if writes succeed locally but later fail upstream?

Yes, and you have to design for it. Two common patterns: (a) tag failed writes
in the cache (via `dio.invalidate_record`) so the UI shows them as conflicted
and the user resolves manually; (b) drop the local change and re-fetch the
upstream version, accepting "last write loses" for sync conflicts. Pick based
on your domain.

## Scenario 3 — API server, read-heavy

A REST API serves a high-traffic product catalog. The underlying database is
correct but slow under load. You want every GET to be served from a shared
in-memory cache with explicit TTL; writes go through normally and invalidate
the cache.

**Strategy.** No persistent cache (server is stateless). In-memory cache
backend. Short TTL. Writes invalidate by id; bulk operations invalidate
wholesale.

```rust
use vantage_diorama::cache::MemorySource;

let lens = Lens::new()
    .cache_source(Arc::new(MemorySource::new()))           // ephemeral
    .with_default(LensDefaults {
        cache_ttl: Some(Duration::from_secs(60)),
        on_start_blocking: false,
        ..Default::default()
    })
    .on_query(|dio, query| async move {
        let rows = dio.master().with_query(query).list_values().await?;
        dio.cache().insert_values(rows).await?;
        Ok(())
    })
    .on_write(|dio, op| async move {
        dio.master().apply(&op).await?;
        if let Some(id) = op.target_id() {
            dio.invalidate_record(id);
        } else {
            dio.invalidate_all();
        }
        Ok(())
    })
    .build()
    .await?;
```

`on_query` fires on the first read for a given query shape; subsequent reads
hit the cache. `cache_ttl: 60s` ensures stale data ages out automatically.

### Why not skip the cache entirely for an API?

If your database can sustain your traffic, you should. The Lens still helps
when traffic spikes past sustainable load — a 60-second cache means a million
identical queries become 17 database hits. Use it when load shedding matters,
skip it when freshness matters more than throughput.

## Scenario 4 — Edge function, ephemeral, no disk

A serverless edge function handles requests with a few hundred KB of memory
and no persistent disk. Cold starts wipe state. You want to amortize the cost
of fetching reference data (product catalogs, feature flags, rate tables)
across many requests within a single instance's lifetime.

**Strategy.** In-memory cache. Eager `on_start` blocks the first request. No
refresh task — the instance is short-lived enough that TTL plus restart
handles staleness.

```rust
let lens = Lens::new()
    .cache_source(Arc::new(MemorySource::new()))
    .with_default(LensDefaults {
        cache_ttl: Some(Duration::from_secs(300)),
        on_start_blocking: true,
        ..Default::default()
    })
    .on_start(|dio| async move {
        let data = dio.master().list_values().await?;
        dio.cache().insert_values(data).await?;
        Ok(())
    })
    // No refresh — instance dies before staleness matters.
    .build()
    .await?;
```

The first invocation pays the cold-start cost; every subsequent invocation in
the same instance reads from memory. When the instance dies, the cache dies
with it; the next instance pays the cost again.

### Should I share a Lens across instances via Redis?

You can. Substitute the cache_source with a Redis-backed `TableSource`. The
trade-off is the network hop on every cache read — usually faster than the
backing store but not free. For edge workloads, in-memory often wins because
the working set is small and instances are short-lived.

## Scenario 5 — Batch pipeline, write-behind

A data pipeline reads from a slow upstream API, transforms records, and writes
to a target warehouse. The pipeline runs for hours; restarts must resume from
where the last run left off; writes are batched for throughput.

**Strategy.** Persistent cache as the work log. Reads from master populate the
cache; writes are buffered and flushed periodically. No reactive surface
needed.

```rust
let lens = Lens::new()
    .cache_at("./pipeline-state.redb")
    .with_default(LensDefaults {
        write_queue_capacity: 4096,
        on_start_blocking: true,
        ..Default::default()
    })
    .on_start(|dio| async move {
        let cursor = dio.cache().get_metadata("last_cursor").await?.unwrap_or_default();
        let new_rows = dio.master().fetch_after(cursor).await?;
        dio.cache().insert_values(new_rows).await?;
        Ok(())
    })
    .on_write(|dio, op| async move {
        // Buffer until a thousand have accumulated, then flush.
        let buffer = dio.cache().scope("pending_writes");
        buffer.apply(&op).await?;
        if buffer.count().await? > 1000 {
            let batch = buffer.drain_all().await?;
            dio.master().bulk_apply(batch).await?;
        }
        Ok(())
    })
    .build()
    .await?;
```

Restarts pick up at `last_cursor` because the cache survives. The pending
writes scope survives too — if the pipeline crashes mid-batch, the next run
resumes the flush.

### When is this overkill vs a plain Vista loop?

If your pipeline runs in one shot and never restarts, use Vista directly. The
Lens earns its keep when you have multi-hour runs, partial failures, or
genuinely need read-side caching to amortize upstream cost across the run.

## Scenario 6 — Realtime UI with push events

A trading dashboard. Prices update via a websocket. Edits to local positions
are rare but must reflect server confirmation. The UI must update within
milliseconds of any price change.

**Strategy.** Combine `on_event` (websocket-driven invalidation) with an
optional `on_refresh` (periodic fallback in case events are dropped). Writes
go through with explicit optimistic UI patches.

```rust
let lens = Lens::new()
    .cache_at("./trading.redb")
    .on_start(|dio| async move {
        let data = dio.master().list_values().await?;
        dio.cache().insert_values(data).await?;
        Ok(())
    })
    .on_event(|dio, evt| async move {
        match evt {
            ChangeEvent::Updated { id, new } => {
                dio.cache().replace_record(&id, new.clone()).await?;
                dio.patched(id, new);                     // fans out to Sceneries
            }
            ChangeEvent::Inserted { id } | ChangeEvent::Deleted { id } => {
                dio.refresh().await?;                     // simpler than diffing
            }
            ChangeEvent::Invalidated => {
                dio.refresh().await?;
            }
        }
        Ok(())
    })
    .on_write(|dio, op| async move {
        // Apply optimistically, then confirm.
        dio.cache().apply(&op).await?;
        match dio.master().apply(&op).await {
            Ok(()) => Ok(()),
            Err(e) => {
                // Roll back the optimistic patch.
                dio.refresh().await?;
                Err(e)
            }
        }
    })
    .refresh_every(Duration::from_secs(60))               // safety net
    .build()
    .await?;

// Wire the websocket into the event_bus.
let dio = lens.make_dio(prices_vista);
let dio_for_ws = dio.clone();
tokio::spawn(async move {
    let mut ws = price_websocket().await?;
    while let Some(msg) = ws.next().await {
        dio_for_ws.handle_event(msg.into()).await?;
    }
});
```

`dio.handle_event(evt)` invokes the registered `on_event` callback. The
callback decides what to do with the event — patch, refresh, ignore. Sceneries
subscribing to this Dio see updates within a render frame.

### Why the periodic refresh if I already have push events?

Push channels drop messages — connection blips, server restarts, client
backgrounding. The periodic refresh is a low-frequency safety net that
guarantees eventual consistency even if a specific event was missed. For
critical data it's cheap insurance.

## Choosing defaults

A few rules of thumb:

- **`refresh_every`**: pick the interval at which staleness becomes
  user-visible. For an admin grid that's minutes; for a feature flag cache
  it's seconds; for a static catalog it's hours.
- **`cache_ttl`**: only matters for memory caches or when you want disk cache
  entries to age out. Persistent caches usually want no TTL — let your
  callbacks decide.
- **`write_queue_capacity`**: default 256 is fine for UI apps. Pipelines that
  burst writes want a few thousand. If you ever see senders blocking on the
  queue, that's overload — surface it rather than hide it with a bigger
  buffer.
- **`on_start_blocking`**: true for desktop UIs (better than empty grid).
  False for servers (let the first request take the hit; don't block startup
  on possibly-slow upstreams).

## Multiple Lenses

If two domains in your app want genuinely different policies (e.g., a
write-heavy editor and a read-only reporting view), use two Lenses. They can
share the same cache backend or use different ones:

```rust
let edit_lens = Lens::new()
    .cache_source(shared_redb.clone())
    .on_write(complex_write_routing)
    .refresh_every(Duration::from_secs(30))
    .build()
    .await?;

let report_lens = Lens::new()
    .cache_source(shared_redb)
    .on_start(eager_load)
    .refresh_every(Duration::from_secs(3600))
    .build()
    .await?;
```

Dios under different Lenses are independent. Cross-Lens invalidation, if you
need it, is done explicitly in callbacks.
