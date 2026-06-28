# Changelog

## 0.6.8 — 2026-06-28

- Formatting only (`cargo fmt --all`); no functional change.

## 0.6.7 — 2026-06-28

- `Dio::get_ref(relation, row)` traverses a reference and returns a new `Dio`
  bound to the narrowed target Vista — mirroring `Table::get_ref` and
  `Vista::get_ref`. The target loads through the normal cache-first,
  failure-tolerant scenery, so a temporarily-unreachable relation source yields
  an empty/stale-but-recovering grid rather than an error; the only synchronous
  failure is a structurally-undefined reference. `Dio` is persistence-agnostic —
  it delegates resolution to the master Vista and wraps whatever it returns.
- `Lens::make_dio_as(master, cache_table_name)` builds a Dio with an explicit
  cache table name (`make_dio` now delegates with `master.name()`). `get_ref`
  uses it to give each parent's traversed target an isolated cache table, so one
  parent's refresh can't clobber another's.

## 0.6.6 — 2026-06-27

- Failed auto-refresh no longer reverts the grid to stale cache. The
  `refresh_every` ticker published `Invalidated` even when its `on_refresh`
  callback failed (e.g. the source returned a 503), forcing every open scenery to
  reseed from cache — which dropped rows added since the last good refresh. The
  ticker now delegates to `Dio::refresh()`, the same path manual refresh uses: a
  failed tick announces `Refreshing` but does **not** invalidate, so the painted
  rows survive a transient source error; only a successful refresh updates the
  cache and repaints.

## 0.6.5 — 2026-06-26

- Diagnostics surface. `dio.diagnostics().await` returns a `DioDiagnostics`
  snapshot — cache row count, query-index count, and one `SceneryDiagnostic` per
  open table scenery (its registry key, refcount, row count, and a
  `RowStatusSummary` of fresh/incomplete/pending/failed). New
  `TableScenery::status_summary()` backs it. Reads off the dedup registry
  (nearly free) and prunes dead entries, so a released scenery vanishes from the
  report — the basis for a "Diorama inspector" panel and leak assertions.
- Adaptive polling via an app-activity signal. A new `ActivitySignal`
  (`Active`/`Standby`/`Offline`), shared into a Lens with `.activity_signal(..)`,
  drives the refresh cadence: `refresh_every` is the active interval,
  `.standby_refresh_every(..)` the slower standby one, and `Offline` skips
  polling entirely (resuming on reconnect). One signal, set by the UI from
  window-focus / idle / network state, re-paces every Dio's refresh loop at
  once. Replaces the fixed-interval ticker (semantics preserved: still skips the
  t=0 tick).
- No-flicker reload. The master Vista is now swappable, and `Dio::reload(new_master)`
  re-points a Dio at a freshly-built Vista (e.g. after its VistaFactory reloaded
  the YAML/script) and rebuilds the cache from it — without tearing the Dio down.
  Open sceneries keep their current rows visible until the new data is staged and
  swap atomically on a single trailing `Invalidated`, so a grid never flashes
  empty even when the dataset changes wholesale. `Dio::master()` now returns
  `Arc<Vista>` (was `&Vista`); the `dio.vista()` facade snapshots its schema at
  construction. Stale per-query indexes are dropped on reload.
- `titles_only()` table-scenery projection — the dropdown / autocomplete shape.
  A picker binds to the same `TableScenery` a grid does (visible band →
  `set_viewport`, typeahead → `set_search`), projected to the title columns. On
  an augmented (two-pass) lookup, `titles_only` **suppresses the detail pass**:
  the picker serves the cheap list-pass rows and never pays for per-row
  hydration, so a large lookup opens a picker as cheaply as it lists. A
  `titles_only` picker keys distinctly in the dedup registry from a full grid
  over the same query, so neither inherits the other's hydration.
- Optimistic writes. `Dio::write_optimistic(op)` (and the `patch_optimistic`
  shorthand) stage a write in the cache and announce it (`WritePending`) before
  the write-through runs, so a form edit shows instantly as
  `RowStatus::PendingWrite`. On success it settles to `Fresh` (`RecordChanged`);
  on failure the cache pre-image is restored and the row flips to
  `RowStatus::WriteFailed` (`WriteReverted`) — the view reverts rather than
  keeping a value that didn't save. The edit reflects across every bound scenery
  via the existing `RecordChanged` fan-out. Two new bus events (`WritePending`,
  `WriteReverted`) are handled by both `RecordScenery` and `TableScenery`; the
  previously-unused `RowStatus::PendingWrite` / `WriteFailed` now have producers.
- Soft-refresh sort for augmented (two-pass) grids. Changing a two-pass
  scenery's sort (or search) now rebuilds the ordered index for the new variant
  and **restarts the detail pass for the visible window** — augmentation resumes
  without the user scrolling. Previously `set_sort` dropped two-pass sceneries
  onto the single-pass reseed path, which never re-listed the variant nor
  re-issued the viewport, so hydration silently stalled. The reorder re-seeds
  from cache in one atomic swap (the grid never blanks), a previously-seen sort
  reorders with no refetch, and a scenery that mutates its own sort/search in
  place leaves the dedup registry (it's no longer the shareable canonical view
  for the original query).
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
- Refresh-on-open now hits the server even when the cache seed already filled the range. The
  cache is id-keyed (arbitrary order) but the server applies the query's ordering, so the open
  fetch uses `force_load` to replace the seed with ordered rows — a freshly-opened grid is ordered
  without a manual refresh.
- A chunk load only bumps the generation when it actually changes a visible row. `write_chunk_row`
  skips a slot that already holds the same `Fresh` record, so a refresh that re-fetches
  byte-identical rows no longer triggers a repaint.
- A short page (fewer rows than the requested window) is treated as the end of the set, so the
  grand `total` is derived from the fetch itself (no separate count request). This self-corrects a
  list opened before its rows existed, whose `total` was counted once at 0 and would otherwise
  never grow. `set_search` also drops the stale cached total, since a new query matches a different
  set.

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
