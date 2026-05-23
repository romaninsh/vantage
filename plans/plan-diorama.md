# Diorama — consolidated plan

`vantage-diorama` houses `Lens` (cache-and-callback apparatus), `Dio` (per-entity wrapper), and
three Scenery surfaces (`TableScenery`, `RecordScenery`, `ValueScenery`) that reactive UIs bind to.

## Architecture

A `Lens` is built once per app with a cache backend, lifecycle callbacks (`on_start`, `on_refresh`,
`on_write`, `on_event`, `on_query`), and default policies (TTL, refresh interval, write-queue
capacity). After `.build()`, `lens.make_dio(vista)` returns a `Dio` that owns a cache namespace plus
per-entity machinery (write-queue worker, refresh task, event bus). From a `Dio` you spawn consumer
surfaces: `dio.vista()` returns a richer Vista; `dio.table_scenery()` / `dio.record_scenery(id)` /
`dio.value_scenery()` return reactive subscriptions that bump a `watch::Receiver<Generation>` on
change. `Diorama::overlay(a, b)` and `Diorama::merge(a, b)` are composition primitives.

## Crate layout

```
vantage-diorama/src/
├── lib.rs
├── lens/        (mod, callbacks, defaults, build)
├── dio/         (mod, shell, worker, refresh, event_bus, hot_tier)
├── scenery/     (mod, table, record, value, enriched_record)
├── composition/ (mod, overlay, merge)
├── ops/         (write_op, query_descriptor, change_event)
└── error.rs
```

## Stage map

| Stage                | Status                               |
| -------------------- | ------------------------------------ |
| 1 Skeleton           | Done                                 |
| 1b Schema-on-source  | Done                                 |
| 2 CSV walkthrough    | Done                                 |
| 3 Write + refresh    | Done (on_query deferred)             |
| 4 Event bus          | Done (LiveStream migration deferred) |
| 5 Table scenery      | Done v2                              |
| 6 Record scenery     | Done v1                              |
| 7 Value scenery      | Done v1                              |
| 8 GPUI adapter       | Not started                          |
| 9 Composition        | Not started                          |
| 10 Decommission live | Not started                          |

## Conventions

- Each stage starts with a **discussion phase** — confirm interface/scope before code.
- Tests use `Result<(), Box<dyn Error>>` or `vantage_core::Result<()>`.
- Callbacks borrow `&Dio` and return `Pin<Box<dyn Future + 'a>>`.

## Dependencies on vista

- **Vista stage 4** — driver factories with honest capability flags. Required for `make_dio`.
- **Vista stage 5** — operator vocabulary for conditions. Required for Sceneries to push down
  filters.
- **Vista stage 5b** — pagination, sort, search, aggregates on Vista. Required so Diorama's
  Sceneries can call these methods uniformly.

Diorama stages 1–3 work against current vista (eq-conditions + list/get/insert/count). Stages 5+
need vista stage 5b (`set_pagination`, `add_order`, `add_search`).

---

## Stage 1 — Crate skeleton (Done)

Created crate with type definitions and trait surfaces only — no execution logic. Defined `Lens`,
`LensBuilder`, `LensCallbacks` (five HRTB callback slots), `LensDefaults`, `Dio`/`DioInner`,
`DioShell` (stub `TableShell`), `WriteOp`, `QueryDescriptor`, `ChangeEvent`, `DioEvent`,
`Generation` newtype, empty `TableScenery`/`RecordScenery`/`ValueScenery` trait shapes, composition
placeholders, error enums. Workspace compiles.

---

## Stage 1b — Schema-on-source refactor (Done)

Moved schema ownership from `Vista` struct fields to `TableShell` trait methods (`columns()`,
`references()`, `id_column()`). `Vista` becomes a thin holder for name + source + capabilities. Each
driver's `XxxTableShell` stores its own `VistaMetadata`. `DioShell` forwards to `master.source()`.
Breaking change to `TableShell` — free because vista is pre-release behind a feature flag.

---

## Stage 2 — CSV walkthrough (Done)

First end-to-end: CSV Vista → Lens → `make_dio` → redb cache. `on_start` callback copies master rows
into cache; `dio.vista().list_values()` reads from cache. Added `lens.cache_at(path)` convenience.
`on_start_blocking = true` default. `DioShell` delegates reads to cache; writes return
`Unsupported`. Integration test + `examples/csv_walkthrough.rs`.

---

## Stage 3 — Write and refresh (Done, on_query deferred)

Added `on_write` and `on_refresh` callbacks. `dio.vista().insert(...)` enqueues `WriteOp` via
bounded mpsc; write worker task consumes queue, invokes callback, emits `DioEvent::WriteFailed` on
error. Capability re-derivation: `on_write` registration flips `can_insert/update/delete`. Refresh
timer (skips first tick). `Dio::refresh()` fires callback synchronously. `on_query` callback
registered but deferred to stage 5b. `QueryDescriptor` struct defined.

---

## Stage 4 — Event bus (Done, LiveStream migration deferred)

Wired `tokio::sync::broadcast<DioEvent>` on `DioInner`. `Dio::invalidate_record(id)`,
`invalidate_all()`, `patched(id, record)` publish events. `Dio::handle_event(evt)` invokes
`on_event` callback. `Dio::subscribe_events()` exposes receiver for Sceneries.
`can_subscribe = true` always on Diorama-output Vistas. `LiveStream` trait migration from
`vantage-live` deferred to stage 10. SurrealDB LIVE wiring updated. Integration tests +
`examples/live_invalidation.rs`.

---

## Stage 5 — Table scenery (Done v2)

### Summary

Implemented `TableScenery` — the reactive table surface. v2 ships sparse
`BTreeMap<usize, Arc<EnrichedRecord>>` storage, `total_provider` callback, `on_load_chunk` callback,
and viewport pipeline with debouncing. Three new `DioEvent` variants (`ViewportChanged`,
`RangeLoaded`, `LoadFailed`). `LensDefaults` knobs: `refresh_on_open`, `viewport_debounce`. BDD
coverage for total count, sparse rows, viewport.

### Deferred follow-ups

- **Targeted single-row updates** — reactor falls back to full reseed on `RecordChanged`; need to
  preserve chunk-loaded index assignments across `Invalidated`
- **Sort/search push-down** — depends on vista stage 5b; currently runs in-memory across sparse map
- **Persisted sparse map** — restart drops index map; redb `(sort_key, idx) → id` table would
  survive
- **Per-push generation bumps** — currently once per chunk; streaming APIs may need finer
  granularity

---

## Stage 6 — Record scenery (Done v1)

### Summary

Implemented `RecordScenery` — single-record reactive surface. Holds one `EnrichedRecord` (or
`None`), exposes status (`Fresh`/`Stale`/`Loading`/`PendingWrite`/`Failed`/`NotFound`), bumps watch
on change. Background task subscribes to event bus, filters by id, re-fetches on match.
`dio.record_scenery(id)` and `dio.record_scenery_with(id, record)` entry points.
`dio.mark_pending_write(id)` tracks in-flight writes. Integration tests for all status transitions.

---

## Stage 7 — Value scenery (Done v1)

### Summary

Implemented `ValueScenery` — single-value reactive surface for aggregates (count, sum, max, min,
custom). Exposes `value() -> Option<CborValue>` + `ValueStatus`. Subscribes to all `DioEvent`s,
recomputes on any change. Falls back to local scan when vista stage 5b aggregates unavailable.
`Aggregate` enum with `Count`, `CountWhere`, `Sum`, `Max`, `Min`, `Custom`. Integration tests +
`examples/badge_demo.rs`.

---

## Stage 8 — GPUI adapter (Not started)

### Discussion phase

- [ ] Adapter crate home: `vantage-ui-adapters/src/diorama/`. Confirm.
- [ ] Bridge entity scope: one per Scenery type — `TableSceneryEntity`, `RecordSceneryEntity`,
      `ValueSceneryEntity`. Explicit structs vs macro? Lean: explicit — clearer for adapter authors.
- [ ] Skeleton cells for unloaded rows — vantage-ui-adapters utility or per-app?
- [ ] Virtual/infinite scroll: gpui-component uses `visible_rows_changed` + `has_more` +
      `load_more`. v1 Scenery is eager (`has_more` = false); adapter wires all hooks for v2 compat.
      Confirm we don't fake `has_more`.
- [ ] Unbounded-local-data escape hatch: adapter ships helper or leave to apps? Lean: leave to apps.
- [ ] Sort interaction: `perform_sort` → `scenery.set_sort`. Confirm `SortDirection::into()` bridge.
- [ ] Quicksearch: adapter ships widget or leaves to apps? Lean: leave to apps.
- [ ] Migration scope: replace one grid in `vantage-ui` as proof, sweep rest later. Confirm.
- [ ] Storybook: ship `MockDio::with_rows(rows)` in test-util for offline fixtures.

### Scope

**In:**

- `vantage-ui-adapters/src/diorama/` — `table_entity.rs`, `record_entity.rs`, `value_entity.rs`
- Reference `TableDelegate` impl in examples consuming `TableSceneryEntity` +
  `gpui-component::DataTable`
- Migrate `vantage-ui`'s `RecordGrid` to `TableSceneryEntity`; detail sheet to `RecordSceneryEntity`
- `MockDio` test utility for storybook
- Integration test: GPUI test app mounts entity, simulates invalidate, asserts re-render

**Out:**

- Other framework adapters (cursive, egui, slint, tauri)
- Sweeping all `vantage-ui` grids
- Quicksearch UI standardization

### Plan

- [ ] Discuss: adapter home, macro vs explicit, migration scope, mock shape
- [ ] Add `vantage-diorama` dep to `vantage-ui-adapters/Cargo.toml`
- [ ] Implement `TableSceneryEntity`: holds `Arc<dyn TableScenery>` + `Task<()>`, spawns watch →
      `cx.notify()`
- [ ] Implement `RecordSceneryEntity` (mirror)
- [ ] Implement `ValueSceneryEntity` (mirror)
- [ ] Reference `TableDelegate`: `rows_count` → `scenery.row_count()`, `render_td` →
      `scenery.row(idx)` match Some/None, `has_more` / `load_more` / `visible_rows_changed` /
      `perform_sort` wired through
- [ ] Migrate `vantage-ui` RecordGrid: replace bulk `list_values()` with `TableSceneryEntity`
- [ ] Migrate detail sheet: `dio.record_scenery(id)` → `RecordSceneryEntity`
- [ ] Add `MockDio` / `MockLens` to test-util
- [ ] Update storybook entries
- [ ] Run integration test
- [ ] Update `README_ui.md` with real code paths

### References

- `../README_ui.md` "GPUI binding pattern" + "worked example end-to-end"
- Touches `../../vantage-vista/plans/8-ui-migration.md` — vista stage 8 migrates bulk model; this
  stage migrates reactive surface

---

## Stage 9 — Composition primitives (Not started)

### Discussion phase

- [ ] `Diorama::overlay(base, overlay)` — reads merge both (overlay wins for shared ids), writes to
      overlay. Confirm.
- [ ] `Diorama::merge(primary, fallback)` — reads try primary then fallback, writes to primary.
      Confirm.
- [ ] `list_values` merge for overlay: option (a) overlay rows replace base by id (full merge).
      Lean: (a).
- [ ] Capability union rules:

  | Capability               | overlay(base, ov) | merge(prim, fb) |
  | ------------------------ | ----------------- | --------------- |
  | can_count                | ov \|\| base      | prim \|\| fb    |
  | can_insert/update/delete | ov.\*             | prim.\*         |
  | can_order/search         | both              | both            |
  | can_subscribe            | either            | either          |
  | can_fetch_page           | false             | false           |

- [ ] Pagination through composition: composed Vistas advertise `can_fetch_page = false`. Users
      needing paginated reads compose at Lens level. Confirm.
- [ ] Conditions propagated to both inner Vistas. Confirm.
- [ ] Live events: subscribe to both inner streams, multiplex. Confirm.
- [ ] Three-way composition: `overlay(a, overlay(b, c))` works because composition produces a Vista.
      Confirm.

### Scope

**In:**

- `Diorama::overlay(base, overlay) -> Vista` and `Diorama::merge(primary, fallback) -> Vista`
- `OverlayVista` / `MergeVista` with `TableShell` impls
- Capability union at construction time
- Condition propagation to both inner Vistas
- Integration tests: overlay(read_only_csv, in_memory), merge(local_cache, remote_api), nested
  composition, capability flags
- Examples: `overlay_csv.rs`, `merge_cache_remote.rs`

**Out:**

- Cross-Vista cache coherency
- Three-way merge with conflict resolution
- Distributed composition

### Plan

- [ ] Discuss: merge semantics, capability rules, pagination policy
- [ ] Implement `OverlayVista` struct + `TableShell` impl
- [ ] Implement `MergeVista` struct + `TableShell` impl
- [ ] Implement constructors returning `Vista`
- [ ] Capability union
- [ ] Condition propagation
- [ ] Live-event multiplex
- [ ] Write integration tests
- [ ] Add examples
- [ ] Update `README_rust_dev.md` composition section

### References

- `../README.md` — composition as one of three Diorama capabilities
- `../README_rust_dev.md` "Composition with other Vistas"

---

## Stage 10 — Decommission vantage-live (Not started)

### Discussion phase

- [ ] Deprecation timing: hard delete vs one-cycle `#[deprecated]` shim. Lean: shim for one cycle.
- [ ] Feature parity audit:

  | vantage-live               | Diorama                            |
  | -------------------------- | ---------------------------------- |
  | `LiveTable::new`           | `lens.make_dio(master)`            |
  | `Cache` trait              | redb-backed cache Vista            |
  | `MemoryCache`              | `MemorySource` cache backend       |
  | `with_custom_write_target` | `on_write` callback                |
  | `with_live_stream`         | `on_event` + manual stream forward |
  | `LiveStream` trait         | moved to diorama (stage 4)         |
  | `LiveEvent`                | `ChangeEvent`                      |

- [ ] In-tree consumers: `vantage-surrealdb` (migrated stage 4), `vantage-ui` (verify direct dep),
      examples/docs
- [ ] Out-of-tree consumers: likely zero; deprecation cycle catches any
- [ ] `live_demo.rs` example + helper script: migrate to diorama examples? Lean: migrate.

### Scope

**In:**

- Migrate remaining in-tree consumers to `vantage-diorama`
- Delete `vantage-live`'s `live_table/` and `cache/` modules
- Optional one-cycle re-export shim with `#[deprecated]`
- Move `live_demo.rs` to `vantage-diorama/examples/`
- Update `TODO.md`, `CHANGELOG.md`, root `README.md`

**Out:**

- New Diorama features — pure cleanup
- Decommissioning vista's `AnyTable` (vista stage 9)

### Plan

- [ ] Discuss: deprecation cycle, example migration
- [ ] Audit in-tree consumers
- [ ] Write Diorama equivalent for each consumer
- [ ] Migrate `live_demo.rs`
- [ ] Decide hard delete vs shim; execute
- [ ] Update root TODO, CHANGELOG
- [ ] Sweep `bakery_model3/examples/` and `example_*/` for vantage-live refs
- [ ] Update `vantage-vista/plans/9-decommission.md` — tick vantage-live bullet

### References

- Closes `../../TODO.md` "Wire up real LIVE query support" remaining sub-bullets
- Touches `../../vantage-vista/plans/9-decommission.md`
