# Stage 3 — Writes and refresh

Status: **Not started**

Add the two remaining "lifecycle" callbacks: `on_write` and `on_refresh`.
At the end of this stage, `dio.vista().insert(...)` enqueues a `WriteOp`
that the user-supplied `on_write` callback consumes; a refresh timer
fires `on_refresh` on the configured interval and on demand via
`dio.refresh().await`.

This stage also adds `on_query` — the cache-miss callback that lazy-load
scenarios depend on (README scenarios 2 and 3).

## Discussion phase

- [ ] Write queue capacity — `LensDefaults::write_queue_capacity = 256`
      is the placeholder. Confirm. (Mirrors vantage-live.)
- [ ] Write queue backpressure: full queue blocks the caller. Confirm
      this is the right surface vs. dropping or erroring.
- [ ] Write worker error handling: callback errors logged via
      `tracing` and emitted as `DioEvent::WriteFailed` on the bus. The
      Dio survives. Confirm: do we also propagate to the caller? No —
      writes are fire-and-forget from `dio.vista().insert(...)`. If
      the user needs synchronous confirmation, they call
      `dio.master().insert(...)` directly.
- [ ] `dio.vista().insert(...)` return type: returns `Result<()>`
      synchronously after enqueuing. If queue is full it blocks until
      space available. Confirm.
- [ ] Capability re-derivation: with `on_write` registered, `DioShell`
      reports `can_insert: true && can_update: true && can_delete: true`
      regardless of master's flags. Confirm.
- [ ] Refresh timer behavior: skip the immediate tick (so the timer
      doesn't fire instantly after `make_dio` when `on_start` just
      ran). Confirm — same pattern vantage-live uses.
- [ ] Manual refresh: `dio.refresh().await` fires the callback
      synchronously and returns its result. Errors propagate. Confirm.
- [ ] `on_query` callback shape: receives `(&Dio, QueryDescriptor)`.
      `QueryDescriptor` carries the current conditions, sort, search,
      pagination state — enough for the callback to fetch the right
      slice from master. Open question: what's the exact field set?
      Lean: clone the cache vista's request state at query time and
      pass that.
- [ ] When does `on_query` fire? `DioShell::list_vista_values` calls
      it if the cache returns empty for the current query AND `on_query`
      is registered. Once fired, re-read cache and return. Risk: thundering
      herd if many readers hit empty cache simultaneously — gate with a
      per-query mutex. Confirm.

## Scope

In:

- `WriteOp` becomes load-bearing (was placeholder in stage 1)
- `LensBuilder::on_write(F)` accepts an async closure boxed via the
  HRTB callback type
- Write queue (`mpsc::Sender<WriteOp>`) on `DioInner`; bounded by
  `LensDefaults::write_queue_capacity`
- Write worker task spawned in `make_dio`; consumes queue, invokes
  `on_write(&dio, op)`, logs errors, emits `DioEvent::WriteFailed`
- `DioShell::insert_vista_value` / `update_vista_value` /
  `delete_vista_value` / `replace_vista_value` enqueue `WriteOp`s
- Capability re-derivation reflects `on_write` registration
- `LensBuilder::on_refresh(F)` accepts an async closure
- `LensBuilder::refresh_every(Duration)` sets the interval
- Refresh task spawned in `make_dio` when interval is set
- `Dio::refresh()` async method — fires `on_refresh` synchronously
- `LensBuilder::on_query(F)` accepts an async closure receiving
  `(&Dio, QueryDescriptor)`
- `DioShell::list_vista_values` fires `on_query` on empty cache
- `QueryDescriptor` struct with current conditions, sort, search,
  pagination
- Integration tests:
  - Write-through: write via `dio.vista().insert`, on_write fires,
    master updated, cache updated by user's callback
  - Refresh: refresh fires on timer, on_refresh updates cache, reads
    see new data
  - On_query lazy fill: cold cache + on_query → reads return master
    data via cache after callback runs
- Example in `examples/write_through.rs`

Out:

- Live events from external sources (stage 4)
- Coalescing of pending writes (stage 10 / future)
- Optimistic UI patches with rollback (Scenery responsibility — stages
  5–6)
- Sceneries subscribing to write completion (stage 4 — needs the bus)

## Plan

- [ ] Discuss with user: queue backpressure, error propagation,
      `on_query` semantics, capability re-derivation rules
- [ ] Implement `LensBuilder::on_write(F)` — store as
      `Box<dyn for<'a> Fn(&'a Dio, WriteOp) -> ... + 'a>`
- [ ] Implement `LensBuilder::on_refresh(F)` — same pattern
- [ ] Implement `LensBuilder::on_query(F)` — receives
      `(&Dio, QueryDescriptor)`
- [ ] Implement `LensBuilder::refresh_every(d)` — store on
      `LensDefaults`
- [ ] Implement write queue: `mpsc::channel(capacity)` allocated in
      `make_dio`
- [ ] Implement write worker task: loop over `rx.recv().await`,
      invoke callback, log errors, emit `DioEvent::WriteFailed` (the
      event bus skeleton is stubbed here; full broadcast wiring lands
      in stage 4)
- [ ] Implement `DioShell::insert_vista_value(record)` — build
      `WriteOp::Insert(record)`, send to queue, return `Ok` when
      enqueued. Note: this changes Vista contract — `insert` no longer
      necessarily means master saw it. Document.
- [ ] Implement `DioShell::update_vista_value`, `delete_vista_value`,
      `replace_vista_value` similarly
- [ ] Capability re-derivation: read `lens.callbacks` at
      `DioShell::capabilities()` time; flip flags if `on_write` is
      `Some`
- [ ] Implement refresh task: `tokio::time::interval` if
      `defaults.refresh_interval.is_some()`; skip first tick; loop
      calling `on_refresh`
- [ ] Implement `Dio::refresh()` — fire callback synchronously
- [ ] Implement `DioShell::list_vista_values`:
  - Read from cache
  - If empty AND `on_query` registered: acquire per-query mutex,
    re-check cache, if still empty fire `on_query`, re-read
  - Return rows
- [ ] Define `QueryDescriptor` fields (defer field exact list to the
      discussion phase; minimum: `conditions`, optional `sort`,
      optional `search`, optional `pagination`)
- [ ] Write integration tests:
  - Write-through (CSV master + redb cache + on_write that writes to
    both)
  - Periodic refresh (mock master that increments a counter; on_refresh
    pulls; assert cache reflects new value after interval)
  - Lazy fill (cold cache; on_query fetches first page; subsequent
    reads served from cache)
  - On_write error path: callback returns Err → `DioEvent::WriteFailed`
    emitted; Dio still functional for next write
- [ ] Update `examples/csv_walkthrough.rs` (from stage 2) to add a
      write demo
- [ ] Add `examples/write_through.rs` showing the full pattern with a
      mock master that records what it saw

## References

- Subsumes:
  - `../README_rust_dev.md` "API endpoint backed by a slow remote"
    scenario — first scenario fully exercisable after this stage
  - `../README_lens.md` scenarios 1, 3, 4 — all need on_refresh /
    on_write working
- Touches:
  - `../../vantage-live/src/live_table/worker.rs` — pattern to follow
    for the write worker shape; we don't reuse the code but the
    mpsc + spawn pattern is the same
