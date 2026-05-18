# Stage 5 — TableScenery

Status: **Done (v1 — eager load, no prefetch/hot-tier/push-down — see module doc)**

## v2 follow-up: windowed Scenery for virtual/infinite scroll

The v1 trait already exposes `set_viewport(range)`, `request_load_more()`,
`has_more()`, and `estimated_total()` — they're accepted but no-op today.
v2 turns them into a real virtual-scroll backend so the Scenery scales to
caches that don't fit comfortably in `Vec<Arc<EnrichedRecord>>` (low
millions of rows / hundreds of MB).

Sketch:

- Replace the dense `Vec<Arc<EnrichedRecord>>` with a sparse
  `Vec<RowSlot>` (`Loaded` / `Pending` / `Empty`). `row_count` reflects
  the cache's full filtered set; `row(idx)` returns `Some` for `Loaded`
  and `None` (= render skeleton) otherwise.
- Background fetcher consumes prefetch requests from a channel. The
  Scenery's `set_viewport(range)` enqueues prefetch for `range ± margin`;
  `request_load_more` extends the loaded frontier.
- `has_more()` returns `true` while the cache's count exceeds what's
  loaded; flips to `false` when the loaded set covers the filtered total.
- Cache-side iteration: needs `CacheTable` to grow a paged scan
  (`list_values_paged(offset, limit)` or `scan(cursor) -> (rows, next)`).
  Until then the v2 fetcher reads `list_values()` and slices.
- Sort/search push-down: when vista stage 5b lands, the Scenery delegates
  to `dio.vista().add_order(...)` and `dio.vista().add_search(...)`
  through `DioShell`, which in turn calls the cache's `add_order` /
  `add_search` if the cache backend honours them. The Scenery doesn't
  need to know whether push-down happened — it just reads paged results.

The v2 implementation is also what unlocks the "insight into local
cached data" pattern described in `../README_ui.md` § "Virtual / infinite
scroll" — today that pattern requires bypassing the Scenery and binding
the UI to `dio.cache()` directly; v2 puts the Scenery back in charge.

Implement the first reactive surface: `TableScenery`. A Scenery
subscribes to a Dio's event bus, maintains an in-memory row vector
sized to the current query + viewport, and bumps a
`watch::Receiver<Generation>` whenever the visible rows change. The UI
adapter (stage 8) bridges that watch into framework-native render
triggers.

This is the longest stage. It introduces the hot tier (moka) and the
background fetcher pattern that Records and Values will reuse in
stages 6 and 7.

## Discussion phase

- [ ] `TableScenery` trait final shape — see `ARCHITECTURE.md` draft.
      Confirm: `row_count`, `has_more`, `estimated_total`, `row(idx)`,
      `set_viewport`, `request_load_more`, `request_refresh`,
      `set_search`, `set_sort`, `subscribe`.
- [ ] Hot tier choice: `moka::future::Cache<RecordId,
      Arc<EnrichedRecord>>`. Confirm `moka` is the right pick over a
      hand-rolled `DashMap` + TTL sweeper. Lean: moka — production-
      grade, async-aware, gets us LRU/LFU/TTL for free.
- [ ] Hot tier scope: one per Dio (not one per Scenery). Sceneries
      share the same row-id-keyed hot tier within a Dio. Confirm.
- [ ] Row storage shape — `Vec<RowSlot>` where
      `RowSlot = Loaded(Arc<EnrichedRecord>) | Pending | Empty`. The
      vector is sparse for unloaded ranges. Confirm.
- [ ] Background fetcher: one task per Scenery. Consumes prefetch
      requests from a channel; updates `rows` vector; bumps
      generation. Confirm.
- [ ] `set_viewport(range)` semantics: debounce internally (e.g.
      coalesce viewport changes that happen within 50ms). Confirm.
- [ ] `request_load_more` idempotency: if a load-more is already in
      flight, subsequent requests are no-ops. Confirm.
- [ ] Sort and search push-down vs local: Scenery asks the
      Dio.vista() for results with the new sort/search; the
      `DioShell::list_vista_values` decides whether to push down
      (cache.add_order if cache supports it) or scan in memory. Open
      question: does the Scenery need to know whether push-down
      happened, or is "Scenery just calls vista" the abstraction we
      offer? Lean: Scenery doesn't know; vista handles it.
- [ ] Page size + pagination: Scenery's `page_size` is a hint that
      drives how many rows are fetched per round-trip. The cache
      stores everything; pagination is purely a load-amortization
      knob. Confirm.
- [ ] Eager mode (load everything upfront, no pagination): is this a
      Scenery flag (`.eager()`) or a separate Scenery type? Lean:
      flag — same trait, different fetcher policy.
- [ ] `EnrichedRecord` shape from stage 1: `record`, `status`,
      `dirty_fields`, `fetched_at`. Confirm. RecordScenery will use
      `dirty_fields` in stage 6; we just need the field on the type
      now.

## Scope

In:

- `TableScenery` trait implementation
- `TableSceneryBuilder` with `.where_eq(...)`, `.sort(...)`,
  `.search(...)`, `.page_size(...)`, `.eager()`, `.open() -> Arc<dyn
  TableScenery>`
- `TableSceneryState` internal struct with row vector, viewport,
  search/sort settings, generation counter, watch sender, fetcher task
- Hot tier (`moka::future::Cache`) on `DioInner` — single per Dio
- Background fetcher task per Scenery
- `EnrichedRecord` becomes load-bearing
- Event bus consumption: Scenery's fetcher subscribes to
  `dio.subscribe_events()`, reacts to `RecordChanged { id }` /
  `Invalidated` / `Refreshing`
- Integration tests:
  - Open Scenery, assert initial row_count matches cache
  - set_viewport triggers prefetch; assert rows fill in
  - request_load_more extends the frontier
  - External invalidate_record bumps generation, row re-fetched
  - set_search filters the row vector and emits new generation
  - set_sort changes the row order and emits new generation
- Example: `examples/scenery_basic.rs` — text-mode "render loop" that
  polls `row(idx)` on every generation bump (no UI yet, but proves
  the contract)

Out:

- UI adapter (stage 8)
- RecordScenery (stage 6) and ValueScenery (stage 7) — separate stages
- Sort/search push-down honesty cleanup — depends on vista stage 5b
  landing
- Optimistic-write status propagation (`RowStatus::PendingWrite`) —
  needs stage 6 for the form-edit story to be complete

## Plan

- [ ] Discuss with user: trait shape final, hot tier, sort/search
      push-down semantics, page size handling
- [ ] Pull `moka` into `vantage-diorama/Cargo.toml`
      (`moka = { version = "X.Y", features = ["future"] }`)
- [ ] Add `hot_tier: Arc<moka::future::Cache<RecordId,
      Arc<EnrichedRecord>>>` to `DioInner`
- [ ] Wrap `EnrichedRecord` construction: `from_record(record)` →
      sets status `Fresh`, no dirty fields
- [ ] Implement `TableScenery` trait on a concrete
      `TableSceneryImpl: Arc`-cloneable
- [ ] Implement `TableSceneryBuilder` with all setters
- [ ] Implement `.open()` — spawns the background fetcher task,
      returns `Arc<dyn TableScenery>`
- [ ] Implement the fetcher task:
  - Subscribe to `dio.subscribe_events()`
  - Loop: select on (event bus, viewport change channel, manual
    refresh/load_more channel, search/sort change channel)
  - On viewport change: prefetch rows in `viewport ± margin` if not
    already loaded; bump generation when each batch lands
  - On `RecordChanged { id }`: if id is in current row vector,
    re-fetch single row via `dio.vista().get_value(id)`, update slot,
    bump generation
  - On `Invalidated`: clear row vector, refetch first page; bump
    generation
  - On search/sort change: clear row vector, refetch
- [ ] Implement viewport debouncing (50ms window)
- [ ] Implement load_more idempotency (atomic "in flight" flag)
- [ ] Capability gating: if `dio.vista().capabilities().can_order` is
      false and the Scenery sets a sort, the fetcher loads all rows
      and sorts in memory (this works because Diorama's
      `dio.vista()` always reports cache's capabilities, which redb
      handles via indexed columns)
- [ ] Write integration tests against a CSV-backed Dio with redb
      cache:
  - Basic: open scenery, row_count > 0 after on_start, every row()
    returns Some
  - Viewport: set_viewport(0..10), assert first 10 loaded
  - Load more: request_load_more bumps row_count by page_size
  - Live invalidate: dio.invalidate_record(id) → scenery's row(idx)
    returns updated record
  - Sort: set_sort("price", Desc) → row(0).price is the max
  - Search: set_search("cake") → row_count drops to matching rows
- [ ] Write `examples/scenery_basic.rs` — text-mode render loop
- [ ] Document the Scenery trait shape on `../README_ui.md`'s "The
      three Scenery types" section (already drafted; verify
      consistency)

## References

- Subsumes:
  - `../README_ui.md` "TableScenery" section — code becomes real
- Pairs with:
  - `../../vantage-vista/plans/5b-query-controls.md` — Vista's
    `set_pagination` / `add_order` / `add_search` are what the
    Scenery's `set_sort` / `set_search` push down through
- Touches:
  - `../../vantage-ui-adapters/` — preparation; stage 8 lands the
    actual GPUI binding
