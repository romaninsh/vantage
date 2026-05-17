# Stage 4 — Event bus and live events

Status: **Not started**

Wire the broadcast event bus that Sceneries will subscribe to in stages
5–7. Add the `on_event` callback so external live-stream sources
(SurrealDB LIVE, MongoDB change streams, custom websockets) can push
change notifications into Diorama. At the end of this stage, an
external event fires → user's `on_event` callback runs → cache is
updated → bus emits a `DioEvent` that *something* could subscribe to
(no Sceneries yet, but the wiring is ready).

This stage also retires `vantage-live::LiveStream` by moving the trait
into `vantage-diorama`.

## Discussion phase

- [ ] `LiveStream` trait fate: move from `vantage-live` to
      `vantage-diorama`. Identical surface (`subscribe() -> Pin<Box<dyn
      Stream<Item = LiveEvent> + Send>>`). Confirm: do we rename
      `LiveEvent` → `ChangeEvent` (the name we've been using in
      design)? Lean: yes, rename. The full breakdown of who
      uses `LiveStream` today is in
      `../../vantage-live/src/live_stream/mod.rs`.
- [ ] `ChangeEvent` payload: include the new record value on
      `Inserted` and `Updated` if the source provides it, or just
      the id (let on_event fetch)? Lean: optional payload
      (`Updated { id, new: Option<Record<CborValue>> }`). Source-side
      decides — SurrealDB LIVE provides values; polling diff doesn't.
- [ ] Broadcast channel sizing: `tokio::sync::broadcast` has a
      capacity. If a Scenery lags, it gets `RecvError::Lagged` and
      misses events — it has to refresh. Confirm: lag tolerance is
      acceptable as long as Sceneries handle it (they will, by
      bumping generation and re-reading state).
- [ ] `Dio::invalidate_record(id)` / `invalidate_all()` /
      `patched(id, record)` are the user-callable API on `Dio` for
      publishing events into the bus from within callbacks. Confirm
      the verb set is right.
- [ ] How does a `LiveStream` get attached to a Dio? Three options:
      (a) `Dio::with_live_stream(stream)` — per-Dio, after make_dio;
      (b) Lens-level: `lens.with_live_stream_factory(|dio| stream)` —
      automatic per-Dio attachment; (c) explicit `tokio::spawn` in
      user code that forwards events into `dio.handle_event(evt)`.
      Lean: (c) is the most flexible and trivially explicit; offer
      (a) as sugar later if patterns warrant. The user wires it once
      after `make_dio`.
- [ ] `Dio::handle_event(evt)` public method: invokes the
      `on_event` callback with the event. This is what the user wires
      from their live stream. Confirm: it's just sugar over
      `lens.callbacks.on_event(dio, evt).await`.

## Scope

In:

- `tokio::sync::broadcast::Sender<DioEvent>` on `DioInner`
- `LensBuilder::on_event(F)` — async closure receiving
  `(&Dio, ChangeEvent)`
- `Dio::invalidate_record(id)` — publishes `DioEvent::RecordChanged
  { id }`
- `Dio::invalidate_all()` — publishes `DioEvent::Invalidated`
- `Dio::patched(id, record)` — writes to cache and publishes
  `DioEvent::RecordChanged { id }`. Convenience for the common
  pattern in `on_event` callbacks.
- `Dio::handle_event(evt) -> impl Future<Output = Result<()>>` —
  fires `on_event` callback, propagates result
- `Dio::subscribe_events() -> broadcast::Receiver<DioEvent>` —
  pub method Sceneries will use in stage 5+; usable now for tests
- Move `LiveStream` trait from `vantage-live` to `vantage-diorama`;
  rename `LiveEvent` → `ChangeEvent` (with `From` impl for back-compat
  during transition)
- Update `vantage-surrealdb` to depend on `vantage-diorama` for the
  `LiveStream` trait once moved (the SurrealDB LIVE wiring in the
  outstanding TODO lands here too)
- Capability re-derivation: `can_subscribe = true` always on
  Diorama-output Vistas (since the Dio always has a bus, regardless
  of whether the master backs it)
- Integration test: mock `LiveStream` emits events → user's
  on_event invalidates cache → second read returns fresh data
- Integration test: `tokio::spawn` forwards from a `LiveStream`
  into `dio.handle_event(evt)`; cache stays in sync

Out:

- Sceneries actually consuming the bus (stages 5–7)
- Polling-based change synthesis (a user pattern, not a framework
  feature — leave it to user code)
- Multi-Lens cross-event-bus federation (out of scope)
- Lens-level live-stream factory sugar (deferred)

## Plan

- [ ] Discuss with user: trait naming (`LiveStream` vs
      `EventStream`?), event payload shape, attachment pattern,
      broadcast capacity defaults
- [ ] Move `LiveStream` trait into
      `vantage-diorama/src/live_stream/mod.rs`
- [ ] Rename `LiveEvent` → `ChangeEvent`; add a `LiveEvent` type alias
      in `vantage-live` for one cycle to ease external migration
- [ ] Add `tokio::sync::broadcast::Sender<DioEvent>` to `DioInner`;
      capacity defaulted to (e.g.) 1024 with a `LensDefaults` knob
- [ ] Implement `Dio::subscribe_events() -> broadcast::Receiver<DioEvent>`
- [ ] Implement `Dio::invalidate_record`, `invalidate_all`, `patched`
- [ ] Implement `Dio::handle_event(evt)` — invokes `on_event`
      callback if registered, otherwise returns `Ok(())`
- [ ] Implement `LensBuilder::on_event(F)` storage
- [ ] Update `DioShell::capabilities()` to set `can_subscribe = true`
- [ ] Update `vantage-surrealdb` to use `vantage-diorama::LiveStream`
      (and finish wiring the SurrealDB LIVE query support); add the
      `live` feature gate. This is the existing TODO entry
      "Wire up real LIVE query support end-to-end" — close it here.
- [ ] Write integration tests:
  - Mock LiveStream emits `ChangeEvent::Updated { id, new: Some(r) }`;
    `tokio::spawn` forwards into `dio.handle_event(evt)`;
    `on_event` patches cache via `dio.patched(id, r)`;
    `dio.subscribe_events()` receiver sees `RecordChanged { id }`
  - `dio.invalidate_record(id)` publishes the event without any
    callback indirection
  - `dio.invalidate_all()` publishes `Invalidated`
  - Lagged subscriber gets `RecvError::Lagged` and recovers
- [ ] Update `examples/` with a `live_invalidation.rs` example using
      the mock stream
- [ ] Update `../../TODO.md` — tick the "Wire up real LIVE query"
      sub-tree as closed (or partially closed if any sub-bullet still
      pending after this stage)

## References

- Closes:
  - `../../TODO.md` "Wire up real LIVE query support end-to-end"
    full sub-tree (surreal-client, vantage-surrealdb, vantage-live
    demo, helper script, CHANGELOG entries) — most of it lands here,
    the demo example moves to `vantage-diorama/examples/` and the
    helper script can be retired or kept for manual testing
  - The vantage-vista plan's note: "`can_subscribe` — universally
    `false`" — Diorama unconditionally sets `can_subscribe: true` on
    its output Vistas, so this is no longer a per-driver concern
- Subsumes:
  - `../../vantage-live/src/live_stream/` — moves wholesale
- Touches:
  - `../../vantage-vista/plans/4-driver-rollout.md` — the
    `can_subscribe` paragraph references this stage
  - `../../vantage-surrealdb/CHANGELOG.md` — entry for the LIVE
    wiring landing
  - `../../surreal-client/CHANGELOG.md` — entry for the
    notification-routing fix (sub-bullet of the TODO sub-tree)
