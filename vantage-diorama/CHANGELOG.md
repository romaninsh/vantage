# Changelog

## 0.7.2 — 2026-07-22

- New records get a default identity: a servo insert without an id now
  generates a **time-ordered UUID (v7)** and writes it into the record,
  so create forms no longer demand a hand-typed id. Id columns declared
  integral (a SQL `INTEGER PRIMARY KEY`) still require an explicit id —
  the error says so up front instead of surfacing a backend datatype
  mismatch later.
- Docs: drop the intra-doc link to the private `QueuedFlash` in the
  `worker` module docs (rustdoc `-D warnings`).

## 0.7.1 — 2026-07-22

**Flash-pipeline hardening: effective caps, reconcile-while-pending, drain-not-drop**

- **`Dio::write_capabilities()`** — the effective write capabilities, and
  the one gate UI chrome asks before offering add/edit/delete. Master
  caps by default; a registered `on_flash` route lifts all three, because
  the route — not the master — is then the writer (a read-only CSV
  becomes editable, changes landing wherever the route sends them). The
  facade's capability lifting now shares this one definition.
- **Reconcile-while-pending**: a refresh carrying a master snapshot taken
  *before* an in-flight flash can no longer clobber the staged value.
  Rows with a flash in flight are tracked internally; the new
  `Dio::reconcile_value` / `reconcile_values` (the cache write an
  `on_refresh` callback should use for master-copied rows) skip them, as
  do the augment refresh pass (including its vanished-row delete) and
  `ChunkSink::push` (which binds the staged value to the slot instead).
  `patched` stays the ingress for push changes — a live stream is fresh
  by definition; snapshots may be stale, hence the guard. On confirm the
  flash also **re-asserts** exactly the fields it wrote over anything
  that raced into the cache mid-flight, keeping fresher values for
  fields it never touched.
- **Drain, not drop**: every queued flash now carries a strong handle to
  the pipeline, so fire-and-forget writes already accepted keep the Dio
  (master, cache, routes, event bus) alive until they land — dropping
  every external handle no longer discards queued work. The worker then
  exits cleanly on its own.

## 0.7.0 — 2026-07-22

**Servo & ChangeFlash — the outbound half of the photography lexicon**

- **`ChangeFlash` replaces `WriteOp`** as the single outbound currency of the
  write pipeline (breaking). Inbound light is a `ChangeEvent`; outbound light
  is a flash. A flash is frozen at fire time and self-contained: `kind`
  (`Insert | Replace | Patch | Delete | Clear`), `id`, the `patch` (only the
  fields that changed), the `before` pre-image, and the derived `after`
  merge. `Clear` replaces `DeleteAll` and keeps its no-optimism
  special-casing; delete flashes now carry the pre-image for routing and
  audit. `flash.active_record(dest)` binds the merged record to any
  `WritableValueSet` — take the change, re-bind it to wherever it should
  land, `save()`.
- **`on_flash` replaces `on_write`** (breaking): the Lens write route now
  receives the full `ChangeFlash`. Registering a route still lifts
  `can_insert/can_update/can_delete` on the facade regardless of the
  master's own capabilities.
- **`Servo` — the editing companion**, opened via `dio.servo(id)` /
  `dio.servo_new()`. A form is a servo loop over a record: `data` holds the
  commanded setpoints, `baseline` the measured upstream state, and the dirty
  set is the **error signal**, computed by diff. Untouched fields run in
  continuous tracking — upstream changes update them live and they stay
  clean; touched fields lock and hold, and upstream converging on the
  setpoint releases the lock by itself. `servo.flash()` freezes the error
  signal into an immutable flash carrying only the changed fields and emits
  it through the optimistic path; `revert()` releases setpoints; `status()`
  reports `Tracking | Pending | Failed`; `subscribe()` follows the scenery
  generation-watch contract. A servo holds a strong Dio handle so the write
  pipeline stays alive while a form is open.
- The optimistic entry points moved into the flash lexicon (breaking):
  `dio.flash(change_flash)` (was `write_optimistic`) plus `flash_patch`,
  `flash_insert`, `flash_replace`, `flash_delete` conveniences. The route
  now always receives a flash with its pre-image filled.

## 0.6.22 — 2026-07-22

- New `CappedScenery` — a row-cap decorator over `TableScenery`. Every
  consumer of the trait sees at most `cap` rows (`row_count`, `row`,
  `estimated_total`, `has_more` all bounded); viewports clamp to the cap and
  `request_load_more` stops at it, while refresh, sort, search, and
  subscription delegate to the wrapped scenery. The view a UI-level `limit:`
  produces — capping the scenery instead of the hydration viewport means a
  master without windowed loading is never asked to serve a viewport
  contract it can't.

## 0.6.21 — 2026-07-21

- The Dio shell passes `VistaCapabilities::can_traverse_in_columns` through from
  the master vista unchanged (column traversal is lowered into the master's
  query; the cache neither adds nor removes it).

## 0.6.20 — 2026-07-20

- A detached lens `on_start` seed now announces itself once it lands
  (`notify_dataset_changed`). The seed writes through the raw cache with
  no events, so a scenery that opened over the still-cold cache previously
  rendered zero rows until a manual refresh.

## 0.6.19 — 2026-07-17

- Docs: qualify the `VistaChange` intra-doc link in `Dio::watch` (rustdoc
  `-D warnings` under 1.97).

## 0.6.18 — 2026-07-16

- `Dio::watch()` — transparent live updates. When the master Vista advertises
  `can_watch` it subscribes and pipes each change through the new
  `Dio::apply_change(ChangeEvent)`, which reconciles the cache and fires the
  membership-correct bus event (`RecordInserted` — now with a producer — for a
  new row, `RecordChanged` for an update, `RecordRemoved` for a delete, a full
  refresh for a coarse invalidation). When the master can't push, `watch()` is a
  no-op and any `refresh_every` timer keeps things fresh — the reactive stack
  behaves identically either way. Failure is self-correcting, not silent: a
  change that fails to apply triggers a reconcile, and a dropped subscription
  backs off, re-subscribes, and refreshes rather than freezing the view.

## 0.6.17 — 2026-07-15

**Central augment scheduler, notify_* rename, exclusive sceneries**

- Detail fetches are now scheduled by the Dio, not by each scenery: every
  consumer that wants rows hydrated (a scenery's viewport, a facade read)
  registers a queue with the central augment scheduler. Queues drain
  round-robin, so disjoint viewports interleave fairly; an id already in
  flight is never fetched twice — the one fetch completes every queue waiting
  on it. Dropping a scenery withdraws its queued ids; a fetch already in the
  air completes and lands in the cache. `Lens::augment_workers(n)` sizes the
  worker pool (default 1 — deterministic order).
- Renamed: `Dio::invalidate_record` → `notify_record_changed`,
  `Dio::invalidate_all` → `notify_dataset_changed`, and `DioEvent::Invalidated`
  → `DioEvent::DatasetChanged`. The names now say what happened — values moved
  vs. membership/order changed — instead of prescribing what a consumer should
  invalidate. (`ChangeEvent::Invalidated`, the upstream push event, is
  unchanged.)
- New `DioEvent::RecordLoadFailed { id, error }`: a scheduled detail fetch
  failed. Two-pass sceneries mark the row `RowStatus::LoadFailed` while its
  cheap list columns stay visible; the row is retried the next time a
  viewport reaches it.
- `TableSceneryBuilder::exclusive()` — a scenery that never shares its
  instance (identical opens normally dedup; an HTTP watch connection must own
  its viewport). Still counted in the demand union, still released on drop.
- Facade bounded reads (`get_value`, `fetch_window`) hydrate through the
  scheduler — a facade read racing a scenery's viewport shares its fetches —
  and honour a hand-rolled `on_load_detail` lens callback as well as
  declarative augmentation.
- `RecordScenery` aborts its event-bus task on drop (was leaking one task per
  open until the Dio died).
- `RedbCache` and `CacheBackend::open_table(name)` are exported: applications
  can claim their own named key-value tables inside the Dio's redb file.
- The shared `QueryIndex` dedups ids across sceneries racing the same page
  and within a single incoming page.

## 0.6.16 — 2026-07-05

**Augment reconciliation: no flap, gap rule, column demand, stale-while-refetch**

- Refresh reconciliation no longer flaps augmented columns. The list pass may
  paint a column the augment owns (a folder listing's cheap `size` under the
  augment's real recursive size); comparing it demoted every hydrated row on
  every refresh — the visible 0 → value → 0 flap. Augment-owned columns are now
  excluded from the change comparison: an unchanged list row keeps its augmented
  values and fetches nothing, and a `modified` bump demotes exactly the moved
  row. Regression: refresh with unchanged rows issues zero detail fetches.
- The gap rule: an augmentation fetches ONLY rows where the list leaves at
  least one augment column absent/null (a folder without a recursive size); a
  row the list already completes (a file's own `size`) stubs `Complete` and
  never touches the detail source. Un-enumerable augments (empty merge list =
  "lift all") always count as a gap.
- Stale-while-refetch: a demoted row takes the fresh list values but KEEPS its
  augmented values while the refetch is in flight — the staleness lives
  out-of-band in `CacheStatus::Incomplete` (what drives the detail pass), so a
  hot row's size ticks up in place instead of strobing blank once a second.
  Blank is reserved for rows never filled; the list's null placeholder never
  erases a fill.
- Augment log lines carry `dio=` — the owning dio's master vista name (which
  embeds the listing key). Two completion series on one `key=` with different
  `dio=` values means two dios are augmenting the same path: a bug made
  visible in the log instead of inferred from gap arithmetic.
- Column demand: sceneries gain a `columns()` open parameter declaring which
  columns the view actually shows (`None` = everything — existing callers
  unchanged; demand joins the dedup key). The Dio's active demand is the union
  over its OPEN sceneries, recomputed as views open and close, and the augment
  detail pass runs only while that union intersects the augment columns — a
  tree of folder names never pays for folder sizes; the listing beside it
  (which demands `size`) is what starts the fetches, and closing it stops
  them. One get still merges ALL augment columns; already-merged values stay
  when demand drains (they just stop refreshing), and a demotion under drained
  demand leaves the columns blank until re-demanded. The full trigger is now
  demand ∧ gap ∧ visibility.
- Augment observability: `vantage_diorama::augment` logs `augment completed` at
  info with the augment key (e.g. the folder path) and the merged `k=v` pairs,
  `no detail record` at debug, and `row demoted for augment refetch` at debug
  when a list-field move triggers a refetch — enough to watch, live, which rows
  re-augment and why.

## 0.6.15 — 2026-07-05

**Augment details can be fixed Vista handles**

- `Augmentation` now names its detail source through a `Detail` enum:
  `Detail::Catalog(name)` resolves through the `VistaCatalog` per fetch (the
  config/YAML form — behavior unchanged), and `Detail::Fixed(Arc<Vista>)` takes a
  direct handle for get-only side tables that live in no catalog — e.g. a
  folder-size vista a listing Dio augments its rows from, keyed by a row column
  (`source: Column { from: "path" }`). Read-key fetches use the shared handle;
  narrowing sources rebuild a private instance per row via
  `TableShell::clone_shell` (a fixed detail whose shell isn't cloneable errors at
  fetch time). Everything downstream is unchanged: hydration stays lazy and
  viewport-driven with per-id single-flight, merged columns patch rows in place
  as they land, and refresh reconciliation demotes a row whose list fields moved
  (its `modified` bumped) so the standing viewport refetches the augment. (API
  break: `Augmentation::table: String` → `Augmentation::detail: Detail`.)

## 0.6.14 — 2026-07-02

**Server-side ordering when the master can order**

- A paged scenery whose master reports `can_order` now pushes its sort **down to
  the master** and skips the client-side re-sort. `Dio::fetch_window_ordered`
  clones the master per fetch (`TableShell::clone_shell`), applies the sort with
  `add_order`, and reads the ordered `[offset, limit)` window — the shared master
  is never mutated, so differently-sorted views can't race. When the master can't
  order (or can't be cloned) it fetches native order and the scenery re-sorts over
  the cache, exactly as before. The eager (non-windowed) path is unchanged — its
  cache is unordered, so it still sorts client-side.
- `on_load_chunk` callbacks now receive the requesting scenery's `sort`
  (`Fn(&Dio, Range, Option<(String, SortDir)>, ChunkSink)`) and should fetch via
  `Dio::fetch_window_ordered`. (API break: add the `sort` parameter.)

## 0.6.13 — 2026-07-01

**Client-side sort orders numeric columns numerically**

- A client-side sort on a `Float` (decimal) column now orders by numeric value
  instead of by the value's `{:?}` debug string. The comparator only special-cased
  `Text`/`Integer`/`Bool` and fell through to a lexicographic debug-string compare
  for everything else, so a decimal column sorted like text: `657.96` ranked above
  `1826.19` (because `'6' > '1'`). A single such value would wedge itself into the
  wrong slot of an otherwise-ordered list — most visible on a live-updating,
  decimal-valued grid or chart. `Integer`/`Float` mixes now compare numerically
  too; `NaN` sorts as equal so it can't panic the sort.

## 0.6.12 — 2026-06-29

**Chunk-loaded grids: sort survives refresh, count stays live**

- A client-side sort on a single-pass, chunk-loaded (paged) scenery is now
  re-imposed after every chunk load and refresh. A paged master that can't order
  server-side returns rows in its native order on each refetch; the scenery
  re-sorts the freshly-cached rows instead of snapping back to native order, so a
  user's column sort no longer flickers away on the next poll. (Orders the loaded
  rows — the documented cost of sorting a lazily-paged, non-orderable source.)
- A chunk-loaded refresh now re-invokes `total_provider`, so a row that appeared
  (or vanished) server-side since open grows (or shrinks) the row count instead of
  staying frozen at the open-time total.
- **A refresh repaints once, atomically — no intermediate-frame flicker.** A
  chunk-loaded refresh used to bump the generation twice: once when re-counting
  (start of the refresh) and again after the in-place refetch + re-sort landed,
  with a network round-trip in between. The grid repainted in that gap — a brief
  flash of the new row count against not-yet-refreshed/re-sorted rows. Re-count is
  now silent and the forced refetch carries the single repaint, so the new count
  and the refreshed, re-sorted rows appear together.
- **A reordering refresh no longer duplicates/drops rows.** A paged refresh used
  to re-fetch only the last viewport by absolute offset; if the master's order had
  shifted (e.g. a `-last_updated` order a live source keeps bumping), a row that
  migrated into the viewport was left ALSO sitting at its old slot (a duplicate),
  silently evicting another row. Refresh now re-fetches the whole contiguous
  loaded block containing the viewport, so a reorder reshuffles cleanly.
- **Sort / filter on a dotted (nested) column now works.** A condition or sort on
  a column like `launch_service_provider.name` — whose value lives in a nested CBOR
  `Map` (a REST `?mode=detailed` belongs-to object) — used to look up the literal
  flat key, find nothing, and silently no-op. Dotted keys now resolve into the
  nested map.
- **A client-sorted refresh never flashes the master's native order.** While a
  refresh's in-place refetch landed, each chunk row was stamped straight into the
  *displayed* map at its absolute master offset — so for the window between the
  pushes and the re-sort the grid (which repaints on its own timer, not just on
  generation bumps) could repaint the rows in the master's native order, then snap
  back to the client sort. On a paged scenery with an active sort the displayed map
  is now a pure projection of the cache: the chunk loader fills the cache and the
  post-load re-sort is the sole writer of the visible map, so the order only ever
  transitions atomically between sorted states.

## 0.6.11 — 2026-06-28

**Lens reduced to pure caching**

- The legacy `Lens::augment` / `LensBuilder::catalog` builder API is removed;
  configure two-pass augmentation on the Dio with `Dio::augment(catalog, augs)`.
  The Lens now owns only caching strategy and explicitly-registered callbacks
  (`on_start` / `on_refresh` / `on_load_chunk` / hand-rolled `on_list_page` +
  `on_load_detail`).
- Removed the unwired `Lens::on_query` seam and the no-op
  `TableSceneryBuilder::eager()` v1 compatibility stub.

## 0.6.10 — 2026-06-28

- Local emulation only forces full-set hydration when a condition/sort actually
  targets an **augmented** column (whose value is unknown until a row hydrates).
  A condition/sort on a **native** list-pass column now refines the cheap cached
  rows in place with normal viewport-driven hydration — so a two-pass grid with a
  `default_sort` on a real column no longer hydrates the whole table on open.

## 0.6.9 — 2026-06-28

First step of the "Dio owns query semantics" work: the Dio — not the scenery or
the Lens — defines what a table is (its conditions, order, and augmentation), and
re-plugs capabilities the backend lacks by emulating them over the cache.

**Dio owns conditions and order**

- `Dio::with_condition_eq(col, val)` and `Dio::with_order(col, dir)` set the
  table's base query. Every scenery opened on the Dio inherits them; a view can
  still add its own conditions or override the sort.

**Dio owns augmentation**

- `Dio::augment(catalog, augmentations)` configures the two-pass list/detail/
  refresh (previously `Lens::augment`), so Dios sharing one Lens can enrich
  differently. The Dio records its *augmented columns* — the marker that a
  condition or sort on that column is client-side and must run locally.
- `Lens::augment` still works (delegates to the same passes, now in a shared
  `dio::augment_passes` module) and is retired once consumers move to the Dio API.

**Local filter/sort emulation**

- A two-pass view carrying conditions/sort the master can't push down now refines
  its visible set over the cache: rows are filtered and sorted by their hydrated
  values, and the visible set (not the index) drives `row_count`.
- A predicate on an augmented column hides each row until hydration confirms a
  match, so matches appear progressively. Such a view hydrates the whole listed
  set — the documented cost of filtering/sorting on a client-side column.
- A view with no conditions/sort is unchanged (raw index order, gray rows kept).

**In-memory cache**

- `LensBuilder::cache_in_memory()` + a `MemoryCache` backend: process-local, no
  file, but with the same per-Dio table and per-row `CacheStatus` semantics as
  redb — so ephemeral Dios and tests skip the `TempDir` while still exercising
  real `Incomplete`/`Complete` round-tripping. It yields once per operation so
  schedule-sensitive consumers behave the same as on redb.
- `MemoryCache`, `CacheStatus`, and `CacheTable` are re-exported at the crate root.

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
