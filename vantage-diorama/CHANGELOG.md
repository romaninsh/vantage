# Changelog

## 0.6.3 — unreleased

- Deduplicating scenery registry. Opening a `TableScenery` for a
  `(conditions, sort, search)` that is already live now returns the **same**
  shared `Arc` — one reactor, one cache window, one in-flight fetch — instead of
  standing up a parallel copy, so many widgets over the same query (a grid plus
  its dropdowns, a row shown twice) cost one scenery. Lifecycle is refcounted:
  when the last handle drops, the scenery's tasks are aborted, cancelling any
  outstanding chunk load or two-pass detail hydration — a closing grid stops
  pulling — and the registry entry is evicted. New `Dio::live_table_scenery_count()`
  exposes the registry (seed for the diagnostics surface).
- Scriptable test source. `MockShell` gained live, by-ref dataset mutation
  (`set_record` / `set_field` / `remove_record` / `clear_records`) so a test or
  example can edit the upstream mid-run and have the next read/refresh observe
  it. The BDD harness builds on this with virtual-time latency and counted
  fault injection (`tests/features/source_control.feature`), giving the diorama
  machinery a deterministic stand-in for a slow/failing/mutating transport —
  no network required. Foundation for the upcoming async-native transport.
- Non-blanking refresh for chunk-loaded (paged/lazy) table sceneries. A refresh now
  re-fetches the last viewport in place — fresh rows overwrite the cached slots, and a
  failed refetch leaves the existing rows untouched — instead of clearing the cache and
  waiting for a refill. Previously any refill lag or error (e.g. a slow/`504` page) left
  the visible rows blank while their count survived.

## 0.6.2 — unreleased

Generic augmentation: wire a **second** Vista into a `Dio` to enrich each master row,
loaded one-at-a-time and merged on top. The master is listed cheaply; each visible row
resolves a detail Vista from a `VistaCatalog`, fetches its record, and merges chosen
columns. The detail source may be the same Vista (the former cmd two-pass) or a
different backend entirely (REST master enriched by a cmd script, or vice versa).

- New `augment` module: `Augmentation { table, source, fetch, merge }`, with
  `Source::{Id, Column, Build}` (closures — Rhai is one factory via `lower_augment`) and
  `Fetch::{PerRow, Batched, Custom}`. `AugmentSpec`/`SourceSpec`/`FetchSpec` are the serde
  (YAML) forms; `lower_augment` lowers them.
- `LensBuilder::augment(...)` + `.catalog(...)`. Registering augmentations engages two-pass
  and synthesizes the list, detail, and refresh-reconciliation passes (the reconciliation
  formerly hand-written in the vantage-ui app now lives here). Each pass is synthesized only
  if the caller didn't supply one.
- The synthesized list pass is capability-aware: it pushes the window down via
  `fetch_window` when the master advertises `can_fetch_window`, else lists and windows
  locally — no more whole-set over-fetch on every page where the backend can window.
- Now depends on `vantage-vista-factory` (catalog resolution + per-row narrowing reuse) and
  adds an optional `rhai` feature (forwards to `vantage-vista/rhai`).

## 0.6.1 — unreleased

- The cache shell now passes `can_fetch_window` through from its master Vista, so a cached REST/SQL
  source still advertises random-access windowing.

## 0.6.0 — unreleased

- Coordinated 0.6 release; internal dependencies realigned to 0.6. No public API changes.

## 0.5.7 — 2026-06-07

Two-pass progressive loading for slow data sources: a cheap **list pass** renders
immediately and expensive per-row **detail** hydrates lazily as rows scroll into
view, without blocking the UI. Engages only when a `detail` callback exists —
SQL/CSV/single-pass paths are unchanged.

- Persist `RowStatus` in the detail cache (envelope of `status + cbor body`) and
  add the `Incomplete` variant. The detail table is keyed by Vista name and shared
  across all filter/sort variants, so hydration resumes after restart and a
  `Fresh` id is never re-fetched.
- Per-query ordered index built by the list pass, keyed by `Vista::index_key`,
  decoupled from the detail store; cached per-key on the `Dio` and shared across
  its sceneries.
- Viewport-driven detail pass hydrates absent/`Incomplete` ids per id, merges the
  detail onto the cached list row (list columns survive), flips it to `Fresh`, and
  bumps the generation; ids already `Fresh` are skipped.
- Sequential no-total paging: pages until a short or empty page, which ends paging
  and freezes the estimated total.
- `QueryDescriptor` carries conditions/sort/pagination into the load callback, so a
  server-side filtered/ordered list pass works.

## 0.5.6 — 2026-06-07

- Pass the new `VistaCapabilities::can_build_ref_via_script` flag through the `Dio` cache
  shell, so a scripted-reference-capable master Vista keeps advertising the capability when
  wrapped for caching. Tracks `vantage-vista` 0.5.4. (Version 0.5.5 was already on crates.io
  without this change; 0.5.6 is the release that actually ships it.)

## 0.5.4 — 2026-06-02

- BDD cache assertions (`cache_record_field`, `cache_record_absent`, `cache_row_count`) now poll
  with bounded retry instead of single-shot asserts, fixing flaky mirror-write scenarios on loaded
  CI where `spawn_blocking` (redb) ops can outlast the virtual-time drain window.

## 0.5.3 — 2026-05-31

- `Dio::removed(id)` clears cached rows before publishing `RecordRemoved`.

## 0.5.2 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.5.0 — 2026-05-23

- Bumped to the 0.5 line to track
  [vantage-table 0.5.0](https://docs.rs/vantage-table/0.5.0/vantage_table/)'s opening of the
  `AnyTable` decommission cycle. No code changes beyond the dependency pin.

## 0.4.5 — 2026-05-22

- `TableScenery::master_capabilities()` exposes the master Vista's capability flags. Lets UI
  delegates pick `set_viewport` for random-access masters and `request_load_more` for cursor-only
  ones.
- `VistaCapabilities` re-exported from the crate root.

## 0.4.4 — 2026-05-20

- Viewport loader now edge-anchors the fetch range. When part of the visible window is already
  cached, the loader anchors the fetch at the cached/uncached boundary and grows it by `page_size`
  in the uncached direction — so dragging slowly across a cached region stops re-fetching what's
  already there. Force-load callers (`request_load_more`) still get the exact range they asked for.
- Viewport loader gained `tracing` spans on the `vantage_diorama::viewport` target — debounce
  absorbtion, skip reasons, effective range, fetch latency, and cache-after counts — for diagnosing
  overfetch and stall behaviour in real UIs.
- BDD coverage extended to the four facade write ops the original `write_path.feature` left out:
  `tests/features/v3_replace.feature`, `v3_patch.feature`, `v3_delete.feature`,
  `v3_delete_all.feature`. Each follows the Insert trio — default-route to master, `WriteFailed` on
  `on_write` error, and the `mock` / `sqlite` Mirror outline.
- `OnWriteMode::Mirror`'s `WriteOp::Patch` arm in the BDD harness now reads-modifies-writes the
  cache so the merged row survives an op that omits columns. `cache.insert_value` is a redb
  full-replace, so the previous arm silently dropped fields outside the partial.

## 0.4.3 — 2026-05-20

- `TableScenery` v2: sparse `BTreeMap<usize, Arc<EnrichedRecord>>` storage replaces the dense `Vec`.
  `row(i)` returns `None` for unloaded indices so virtualised UIs can render a skeleton at that
  slot.
- New `LensBuilder` callbacks: `total_provider(&Dio) -> usize` runs once per scenery open and drives
  `row_count` / `estimated_total` ahead of any rows being paged in;
  `on_load_chunk(&Dio, Range, ChunkSink)` fetches uncached ranges, with
  `ChunkSink::push(idx, id, record)` writing to the cache and the scenery's sparse map.
- New `DioEvent` variants — `ViewportChanged`, `RangeLoaded`, `LoadFailed` — fan out
  viewport-pipeline progress without colliding with `Invalidated`. The reactor ignores its own
  events to avoid loops.
- New `LensDefaults`: `refresh_on_open` (default true) re-fetches the first page in the background
  at scenery open; `viewport_debounce` (default 50ms) coalesces rapid scroll bursts into a single
  fetch.
- `TableSceneryBuilder::page_size` default raised from 50 → 100; `.initial_range(range)` overrides
  the refresh-on-open viewport.
- BDD coverage for the three new contracts: `tests/features/v2_total_count.feature`,
  `v2_sparse_rows.feature`, `v2_viewport.feature`.
- `src/scenery/table.rs` split into `scenery/table/{mod,builder,state,loader,reactor,helpers}.rs` so
  the viewport pipeline and reactor can grow without one monolithic file.

## 0.4.2 — 2026-05-19

- BDD harness now covers the full Diorama surface: Lens lifecycle, write path (`on_write` modes,
  `WriteFailed` events, capability lifting), event path (`ChangeEvent` → `on_event` → cache,
  `TableScenery` generation contract), `refresh_every` skip-first semantics under virtual time,
  multi-Dio cache isolation, and read paths against Mock / CSV / in-memory SQLite via a
  `Scenario Outline`.
- Event-sequence assertions now use [insta](https://crates.io/crates/insta) snapshots so contract
  drift shows up in diff review.
- Fixed a latent bug in `bump_generation` across all three Scenery types: `watch::Sender::send`
  silently dropped values when no receiver was momentarily subscribed. Switched to `send_replace` so
  UIs that drop and re-subscribe always see the latest generation.

## 0.4.1 — 2026-05-18

- New BDD test harness under `vantage-diorama/tests/` using
  [cucumber](https://crates.io/crates/cucumber). Scenarios run on a single-threaded paused-clock
  tokio runtime so refresh/timeout behaviour is deterministic — `tokio::time::advance` is the only
  thing that moves the clock.
- Initial features cover the Phase-1 mock-backend wiring (`skeleton.feature`) and the three Lens
  lifecycle contracts (`lifecycle.feature`): `on_start_blocking=true` parks `make_dio`,
  `on_start_blocking=false` lets it return, and dropping the last `Dio` handle shuts the write
  worker down cleanly.
- Added `#[doc(hidden)]` `Dio::take_write_worker_handle` so the harness can await clean worker exit.
  Not part of the supported surface — `#[doc(hidden)]` is the contract.

## 0.4.0 — 2026-05-18

- Initial release. `vantage-diorama` adds a cached, composable, reactive surface in front of a
  `vantage-vista` `Vista`: `Dio::vista()` hands callers a fresh facade Vista that reads through the
  cache, while writes go to the master and re-emit through the event bus.
- Designed to pair with the schema-on-source `TableShell` shape introduced in
  [vantage-vista 0.4.10](https://docs.rs/vantage-vista/0.4.10/vantage_vista/): the facade shell
  forwards `columns` / `references` / `id_column` to the master, so consumers don't see a stale or
  duplicated schema.
- Pre-release: API surface, scenery types, and event-bus semantics will move before 0.5.
