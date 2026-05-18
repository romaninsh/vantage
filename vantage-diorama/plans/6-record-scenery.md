# Stage 6 — RecordScenery

Status: **Done (v1 — cache-as-truth; no master fetch / PendingWrite / dirty_fields)**

Implement the single-record reactive surface. A `RecordScenery` holds
one `EnrichedRecord` (or `None` if not found), exposes status
(Fresh/Stale/Loading/PendingWrite/Failed/NotFound), and bumps a watch
channel when the underlying record changes. Used by the right-aligned
detail sheet in `vantage-ui` and equivalent form/card patterns.

Builds directly on stage 4 (event bus) and stage 5 (hot tier).

## Discussion phase

- [ ] Builder API: `dio.record_scenery(id)` is the entry point.
      Confirm: just an id, or a builder? Lean: just an id — there's no
      query or sort or filter to configure for a single record.
- [ ] Optional pre-loaded record: `dio.record_scenery_with(id,
      record)` — for cases where the parent grid already has the
      record and wants to hand it off without re-fetching. Lean: yes,
      sugar method.
- [ ] Initial state when cache doesn't have the record: returns
      `record() = None`, `status = Loading`, spawns a fetch. Confirm.
      `Loading` clears to `Fresh` when fetch returns; to `NotFound` if
      master returns `Ok(None)`; to `Failed { error }` if master
      returns `Err`.
- [ ] PendingWrite status: when an `on_write` is in flight against
      this record, the RecordScenery sees the optimistic patch
      immediately if the user's callback writes to cache first. Do
      we need an explicit "this write is pending master confirmation"
      status, or does that live in cache row metadata? Lean: a status
      on `EnrichedRecord` that the `on_write` callback can set via a
      Dio convenience method: `dio.mark_pending_write(id)`. Stage 6
      defines the contract; the actual transition during an in-flight
      write is up to user callback logic.
- [ ] `dirty_fields` semantics: the form-edit case. RecordScenery
      holds an `EnrichedRecord` where `dirty_fields` is `Some` while
      the user is editing. When `dio.vista().update(id, patch)` is
      called, the dirty fields clear. Confirm: dirty tracking is
      Scenery-side state, not cache state. The cache always holds the
      saved record.
- [ ] How does the form view express edits? Two options: (a) form
      holds its own draft state and submits an update on save; (b)
      RecordScenery exposes `apply_local_edit(field, value)` that
      updates `dirty_fields` and bumps generation. Lean: (a) — form
      view manages its own draft. RecordScenery just shows the saved
      record + a `dirty_fields` flag indicating "this field has a
      pending local edit." UI reads from form state for the input
      value and from the scenery for "is this dirty against saved?"
- [ ] Subscription scope: RecordScenery only reacts to
      `DioEvent::RecordChanged { id }` matching its id. Other events
      (Invalidated, others) trigger a re-fetch by id. Confirm.

## Scope

In:

- `RecordScenery` trait
- `Dio::record_scenery(id) -> Arc<dyn RecordScenery>`
- `Dio::record_scenery_with(id, record) -> Arc<dyn RecordScenery>`
- `RecordSceneryImpl` internal state: `record: RwLock<Option<Arc<
  EnrichedRecord>>>`, `status: RwLock<RecordStatus>`, generation
  counter, watch sender, event bus subscription task
- Background task subscribes to event bus, filters by id, re-fetches
  on match
- `Dio::mark_pending_write(id)` — sets cache row's status (if cache
  supports per-row metadata; otherwise tracks in DioInner's
  `pending_writes: HashSet<RecordId>`)
- Integration tests:
  - Open record_scenery for an id present in cache: record() returns
    Some, status Fresh
  - Open for an id not in cache: status Loading; spawns fetch; settles
    to Fresh or NotFound
  - External `dio.invalidate_record(id)`: bumps generation, re-fetches
  - External `dio.patched(id, new)`: bumps generation, record() returns
    new
  - Write through `dio.vista().update(id, patch)`: status briefly
    PendingWrite, returns to Fresh after on_write confirms
  - id mismatch: invalidate_record(other_id) does NOT bump this
    Scenery's generation

Out:

- UI adapter (stage 8)
- Field-level dirty tracking primitives in `Record<CborValue>` —
  RecordScenery uses a parallel `dirty_fields: Vec<String>` slot;
  a full Record refactor for dirty tracking is a separate effort
  (noted in the readme)
- Multi-record sheets (master/detail editor) — out of scope

## Plan

- [ ] Discuss with user: builder API, status transitions,
      pending-write semantics, dirty-field tracking ownership
- [ ] Define `RecordStatus` enum (`Fresh`, `Stale`, `Loading`,
      `PendingWrite`, `Failed { error: String }`, `NotFound`)
- [ ] Implement `RecordScenery` trait
- [ ] Implement `RecordSceneryImpl`
- [ ] Implement `Dio::record_scenery(id)` — looks up cache, builds
      impl; if cache miss, status = Loading, spawn fetch
- [ ] Implement `Dio::record_scenery_with(id, record)` — pre-loaded
- [ ] Implement `Dio::mark_pending_write(id)` — flips status on the
      active Scenery if one exists; tracks via a per-Dio
      `pending_writes` set
- [ ] Implement subscription task: subscribe to dio.subscribe_events,
      filter on id, on match re-read cache (or refetch from master),
      bump generation
- [ ] Wire into `DioShell::update_vista_value` (etc.) — on enqueue,
      flip PendingWrite; on worker success, clear via Dio event; on
      worker failure, emit Failed
- [ ] Write integration tests (listed above)
- [ ] Add `examples/sheet_demo.rs` — text-mode render that polls
      record() and status() on every generation bump

## References

- Subsumes:
  - `../README_ui.md` "RecordScenery — the detail sheet" section
- Pairs with:
  - Stage 8 (GPUI adapter) — the right-aligned-sheet pattern in
    `vantage-ui` swaps to RecordScenery there
- Touches:
  - `Record<CborValue>` evolution: dirty-field tracking on the
    underlying `Record` is a future enhancement, not blocked by this
    stage. `dirty_fields: Vec<String>` on `EnrichedRecord` is the
    Scenery-side workaround for now.
