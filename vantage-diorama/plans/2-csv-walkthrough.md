# Stage 2 — CSV walkthrough (first end-to-end)

Status: **Not started**

Wire the skeleton end-to-end against the simplest possible Vista: CSV. Validate the Lens → Dio →
Vista path with a single working callback (`on_start`). At the end of this stage, a CSV file loads
into a redb cache when a Dio is made, and `dio.vista().list_values()` returns those rows from the
cache.

This is the "first driver" stage from vantage-vista's pattern: prove the shape works against one
concrete driver before scaling out.

## Why CSV first

CSV is read-only, has no native pagination, no sort, no filter, no live events. That's perfect for
stage 2 — we exercise only the load-once flow. Nothing the driver does can mask a Diorama bug. If
`on_start` runs and the rows land in redb, the architecture works.

## Discussion phase

- [ ] Cache backend: redb is the default. Stage 1 left `cache_source` generic on
      `Arc<dyn TableSource>`. Stage 2 wires the concrete `Redb::open(path)` factory behind a
      convenience builder method `lens.cache_at(path)`. Confirm: do we ship a `RedbSource` wrapper
      type in this crate, or reuse `vantage-redb`'s `Redb` directly? Lean: reuse
      `vantage-redb::Redb` — it already implements `TableSource`.
- [ ] Cache namespace allocation: `master.name()` is the default table name within redb. Confirm.
      Override path: `make_dio_named(name,     vista)`.
- [ ] How does `dio.cache()` work without callbacks? It's just the redb Vista with
      `table = master.name()`. Confirm we can construct a Vista from `vantage-redb::Redb` against an
      arbitrary table name at runtime, not just at `from_table`/YAML time. May need a small
      `vantage-redb` extension.
- [ ] `on_start_blocking = true` default: confirm `make_dio` awaits the callback before returning.
      Otherwise the first immediate `dio.vista().list_values()` reads an empty cache. (Users who
      don't want blocking can flip the default.)
- [ ] `DioShell::list_vista_values` minimum impl: delegate to `cache.list_values()`. No `on_query`
      fallback yet — if the cache is empty (because `on_start` is fire-and-forget), we just return
      empty. Confirm. On_query lands in stage 3.
- [ ] `DioShell::insert_vista_value` and other writes return `Unsupported` for stage 2 — write queue
      lands in stage 3. Confirm the placeholder error message.

## Scope

In:

- `Lens::cache_at(path) -> LensBuilder` convenience method (constructs a `vantage-redb::Redb`)
- `lens.make_dio(vista) -> Result<Dio>` actually fires `on_start` if registered; awaits or
  fire-and-forget based on `on_start_blocking`
- `Dio::vista()` returns a real `Vista` whose `DioShell` delegates reads to the cache Vista
- `DioShell::list_vista_values` reads from `cache`
- `DioShell::get_vista_value` reads from `cache`
- `DioShell::count_vista_values` reads from `cache`
- `DioShell::capabilities()` reports cache's read capabilities plus inherited `can_count`
- Integration test: typed CSV table → Vista → Lens → make_dio → `dio .vista().list_values()` returns
  rows from the redb cache after `on_start` runs
- Smoke test for missing `on_start`: `dio.vista().list_values()` returns empty (cache cold, no
  callback to fill it)
- One example in `vantage-diorama/examples/csv_walkthrough.rs` exercising the path

Out:

- Writes (stage 3)
- Refresh scheduler (stage 3)
- `on_query` cache-miss callback (stage 3)
- Event bus / on_event (stage 4)
- Sceneries (stages 5–7)

## Plan

- [ ] Discuss with user: cache namespace allocation, redb extension needs, blocking semantics, stub
      behavior for unsupported ops
- [ ] Pull `vantage-redb` and `redb` (latest version) into the `vantage-diorama/Cargo.toml`
- [ ] Add `lens.cache_at(path)` convenience method that constructs a `Redb` and stores it as
      `Arc<dyn TableSource>`
- [ ] Decide: does `vantage-redb` need a `Redb::open_table(name)` method that returns a `Vista`
      against a dynamically-chosen table? If yes, add to `vantage-redb` (patch bump). If no,
      document why not.
- [ ] Implement `make_dio`:
  - Allocate cache table name from `master.name()`
  - Construct cache Vista from `lens.cache_source` at that table
  - Construct `DioInner` with master + cache, empty channels
  - If `on_start` is registered:
    - If `on_start_blocking`: `cb(&dio).await?`
    - Else: `tokio::spawn(cb)` and return immediately
  - Return `Dio { inner }`
- [ ] Implement `DioShell::list_vista_values` — delegate to `self.dio.cache.list_values()`
- [ ] Implement `DioShell::get_vista_value` — delegate to `self.dio.cache.get_value(id)`
- [ ] Implement `DioShell::count_vista_values` — delegate to `self.dio.cache.count()`
- [ ] Implement `DioShell::capabilities()` — read cache's capabilities, union with
      `can_count: true`, set writes false (stage 3 flips these when `on_write` registered)
- [ ] Implement `Dio::master()`, `Dio::cache()` accessors returning `&Vista`
- [ ] Write `vantage-diorama/tests/csv_walkthrough.rs`:
  - Build a CSV Vista from a fixture file
  - Build a Lens with `cache_at("./test.redb")` and an `on_start` that copies all master rows into
    cache
  - `lens.make_dio(csv_vista)` — assert `on_start` ran
  - `dio.vista().list_values()` returns the expected rows
  - `dio.vista().count()` returns the right number
  - Drop the lens, reopen — `dio.vista().list_values()` still returns rows from the persisted redb
- [ ] Write `examples/csv_walkthrough.rs` — same shape, prints output; runs under
      `cargo run --example csv_walkthrough`
- [ ] Document the example in `README_rust_dev.md` as the canonical "minimum useful Diorama"

## References

- Subsumes (preparation):
  - `../README.md` "Status" section's first concrete deliverable
  - `../README_rust_dev.md` "The minimum useful Diorama" — this stage is what makes that example
    real
- Touches:
  - `../../vantage-redb/Cargo.toml` — possibly needs a patch bump for the `open_table(name)` method
  - `../../vantage-vista/plans/4-driver-rollout.md` — CSV Vista shipped (stage 4 of vista), so we
    inherit the working `from_table` path
