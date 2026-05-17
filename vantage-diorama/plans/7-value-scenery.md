# Stage 7 — ValueScenery

Status: **Not started**

Implement the single-value reactive surface. A `ValueScenery` exposes
one `CborValue` (a count, a sum, a max, a single aggregate result, or
any free-form scalar) plus a status, and bumps a watch channel when
the value changes. Used by menu-bar badges, dashboard summary numbers,
and any "one number that refreshes."

Smallest Scenery type. Mostly a thin variant on the RecordScenery
pattern.

## Discussion phase

- [ ] Builder API: `dio.value_scenery()` returns a builder.
      `.aggregate(Aggregate)` is the primary configuration. Confirm.
- [ ] `Aggregate` shape: an enum of common reductions plus an escape
      hatch:
      ```rust
      pub enum Aggregate {
          Count,
          CountWhere(Condition),
          Sum(String),
          Max(String),
          Min(String),
          Custom(Box<dyn Fn(&Vista) -> Pin<Box<dyn Future<Output =
              Result<CborValue>> + Send + '_>> + Send + Sync>),
      }
      ```
      Confirm field names; lock the trait object signature for
      `Custom`.
- [ ] How does the Scenery know when to recompute? Two options: (a)
      subscribe to `DioEvent::*` and recompute on every event; (b)
      let the user push updates via `dio.value_changed("scenery_id")`.
      Lean: (a) — coarse but correct; recompute is cheap for indexed
      aggregates on a redb cache. Users who need finer control wire
      a custom aggregate that gates on `dio.invalidate_record` events.
- [ ] Initial state: status = Loading; compute runs; settles to Fresh.
      Confirm.
- [ ] Failed aggregate: status = Failed; value() returns the last good
      value or None? Lean: returns last good value (so the UI doesn't
      flash). Status separately signals the error.
- [ ] Push-down: aggregates that map to Vista's `get_count` / `get_sum`
      / `get_max` / `get_min` (vista stage 5b) push down to cache via
      `dio.vista()` — and to master if cache passes through. Cache
      handles where it can; otherwise scan. Confirm — same shape as
      TableScenery push-down.

## Scope

In:

- `ValueScenery` trait — `value() -> Option<CborValue>`, `status() ->
  ValueStatus`, `request_refresh()`, `subscribe() ->
  watch::Receiver<Generation>`
- `Aggregate` enum (Count, CountWhere, Sum, Max, Min, Custom)
- `ValueSceneryBuilder` — `.aggregate(Aggregate)`, `.open()`
- `ValueSceneryImpl` with background task subscribed to event bus,
  recomputing on any change
- Vista interaction: pushes aggregates down through
  `dio.vista().get_sum(field)` etc. when vista stage 5b is available;
  falls back to local scan via `list_values` + reduce otherwise
- Integration tests:
  - Count over a CSV-backed Dio with no filter: matches cache count
  - CountWhere with a condition: matches filtered count
  - External invalidate bumps generation and triggers recompute
  - Custom aggregate: user-supplied closure runs and result reaches
    consumer
  - Failed aggregate: error surfaced via status; last good value
    preserved

Out:

- UI adapter (stage 8)
- GROUP BY-style multi-row reductions — out of scope; a Scenery
  per group is the user pattern, or a `MapScenery` future addition
- Aggregate caching/memoization across multiple ValueSceneries with
  the same definition — out of scope; users who care can share the
  Arc

## Plan

- [ ] Discuss with user: Aggregate enum shape, recompute trigger,
      failure semantics
- [ ] Define `ValueStatus` enum (`Loading`, `Fresh`, `Stale`,
      `Failed { error: String }`)
- [ ] Define `Aggregate` enum
- [ ] Implement `ValueScenery` trait
- [ ] Implement `ValueSceneryBuilder`
- [ ] Implement `Dio::value_scenery() -> ValueSceneryBuilder`
- [ ] Implement `ValueSceneryImpl`:
  - Subscribe to event bus
  - On any event: recompute
  - Recompute: dispatch by `Aggregate` variant
    - `Count` → `dio.vista().count()`
    - `CountWhere(c)` → cloned vista + add_condition + count
    - `Sum(f)` / `Max(f)` / `Min(f)` → `dio.vista().get_sum(f)` etc.
      (needs vista stage 5b; until then, scan + reduce locally)
    - `Custom(f)` → `f(dio.vista()).await`
  - Update value if changed; bump generation
- [ ] Write integration tests
- [ ] Add `examples/badge_demo.rs` — text-mode counter that prints
      whenever the value changes
- [ ] Document `ValueScenery` usage in `README_ui.md` (already
      drafted; verify consistency)

## References

- Subsumes:
  - `../README_ui.md` "ValueScenery — counters and badges" section
- Pairs with:
  - `../../vantage-vista/plans/5b-query-controls.md` — `get_sum` /
    `get_max` / `get_min` push-down requires that stage; until then,
    local scan
- Touches:
  - Future "MapScenery" / group-by-keyed value matrix — possible
    follow-up if dashboards demand it
