# Stage 7 — vantage-coop crate

Status: **Not started**

Separate crate. **Coop wraps a Vista and fills in the capabilities the
inner driver doesn't natively provide.** Every Vista method is expected
to work; if the inner driver returns `Unsupported` (paginate on a REST
endpoint that has no limit/offset, insert on a read-only CSV, search on
a key/value store), Coop closes the gap client-side. Coop also layers in
caching, write routing, and live-event invalidation — same mechanism,
different fillers.

The consumer of a Coop'd Vista holds a plain `Vista`. Nothing in the
caller's surface signals "this is wrapped" — only runtime behaviour and
the `capabilities()` flags change.

## Architecture

`Coop::wrap(vista) -> Vista` is the primitive. It boxes the inner Vista
inside a `CoopShell` (which implements `TableShell`) and returns a new
Vista pointing at that shell. The new Vista's metadata (columns,
references, id) is copied verbatim; capabilities are re-derived after
applying whichever Coop fillers are configured.

Builder chain on Coop sets which fillers run:

```rust,ignore
let products = Coop::wrap(vista)
    .with_pagination()           // local slice if driver doesn't paginate
    .with_cache(MemCache::new()) // read-through + ttl + invalidate
    .with_search()               // fetch-and-filter if no native search
    .with_sort()                 // fetch-and-sort fallback
    .with_writes(handler)        // custom write path (log, audit, queue)
    .with_live(stream)           // invalidate cache from notification stream
    .build();
```

Each filler attaches to a specific method on `TableShell`. The Coop
shell's impl for that method picks: native push-down (delegate to
inner) if the inner reports it; client-side fallback otherwise.
Capability re-derivation is mechanical — once `with_writes` is wired,
the produced Vista reports `can_insert: true` even when the inner is
read-only.

A separate `CoopFactory::wrap_factory(inner)` convenience exists for
the "every Vista produced should be Coop'd" use case (e.g. UI grids
that always want pagination), but the primitive is per-Vista.

## Discussion phase

Items deferred from earlier rounds — re-open here.

- [ ] Wrap surface — confirm per-Vista wrap is the primitive, factory
      wrap is the convenience (vs. the inverse). User-stated intent
      points to per-Vista as the primitive.
- [ ] Builder mutability — `Coop::wrap(vista).with_pagination().build()`
      vs. configuring on the wrapper before calling `wrap`. Lean: chain
      on the wrap result, terminate with `build()` that returns the
      outer Vista.
- [ ] Capability transformation rules — table of (inner caps × Coop
      fillers) → outer caps. Document explicitly so consumers reason
      about it without reading source.
- [ ] `with_pagination()` policy — fetch-all-and-slice (simple), or
      windowed buffering with prefetch? Lean: fetch-all-and-slice for
      v1, document the memory cost, note prefetch as a follow-up. Add
      a `max_rows` safety knob to refuse blowing the heap on
      unbounded sources.
- [ ] `with_sort()` policy — same question. Same lean: fetch-all,
      sort client-side, refuse beyond `max_rows`.
- [ ] `with_search()` policy — string contains across `SEARCHABLE`
      columns is the default; pluggable predicate? Lean: simple
      contains for v1, escape hatch closure for power users.
- [ ] `with_writes(handler)` signature — `Fn(WriteOp) -> Result<...>`
      where `WriteOp` is an enum (`Insert`, `Update`, `Patch`,
      `Delete`, `InsertReturnId`)? Or one closure per op? Lean: enum
      with a single closure, keeps registration light.
- [ ] Cache abstraction surface — minimum: `get(key)`, `put(key, value,
      ttl?)`, `invalidate_id(id)`, `invalidate_all()`. Backend-agnostic
      so users can plug Redis/disk themselves.
- [ ] Cache key strategy — auto from `{driver}/{table}/{conditions
      hash}` plus id for record-scope entries. Manual override?
- [ ] Notification demux strategy — single per-connection stream that
      Coop fans out by table name, vs. per-Vista subscription? Decision
      driven by the SurrealDB LIVE wiring.
- [ ] Live invalidation semantics — does a `LiveEvent::Inserted` on
      bakery `clients` invalidate the cached `clients` list and *every*
      conditioned variant of it? Lean: yes (broad invalidation by
      table); narrow invalidation is a follow-up.
- [ ] Capability cascade with composition — Coop'ing a Coop'd Vista.
      Confirm cleanly composes (outer Coop wraps inner Coop's shell)
      and the capability re-derivation is idempotent.

## Scope

In:

- New `vantage-coop` crate
- `Coop::wrap(vista) -> CoopBuilder` (per-Vista primitive)
- `CoopBuilder::with_*` chain and `build() -> Vista`
- `CoopShell` — implements `TableShell` by delegating to the inner
  Vista's shell with per-method fillers
- Capability transformation logic (`VistaCapabilities` recomputed
  after each `with_*`)
- Filler implementations:
  - `with_pagination()` — fetch-all, slice; honours `set_pagination`
    set on the outer Vista
  - `with_cache(impl Cache)` — read-through caching with explicit
    invalidation hooks; reads served from cache when warm
  - `with_search()` — local string-contains across `SEARCHABLE` columns
  - `with_sort()` — local sort by field/direction
  - `with_writes(handler)` — `WriteOp` enum dispatch; converts every
    write call into a user-supplied closure
  - `with_live(stream)` — invalidate `with_cache` from external events
- `Cache` trait + `MemCache` reference impl (in-memory, HashMap-based)
- `LiveStream` trait (port from existing vantage-live or replace)
- `WriteOp` enum + handler signature
- `CoopFactory::wrap_factory(inner)` convenience (every produced Vista
  is auto-Coop'd with the same configuration)
- SurrealDB LIVE-query wiring — subscribed via vantage-surrealdb's
  notification pipe, fed to Coop's invalidation channel
- Capability transformation table in `vantage-coop/README.md`

Out:

- Distributed caches (Redis, etc.) — pluggable trait left for users
- Cross-Vista cache coherency (changes to `orders` invalidating
  `client.with_many('orders')` traversals) — narrow invalidation
  follow-up
- Write-ahead-log persistence
- Optimistic-concurrency / version-stamp semantics on `with_writes`
- Prefetch / windowed pagination — fetch-all only for v1

## Plan

- [ ] Discuss with user: per-Vista vs factory primitive, builder
      mutability, capability table, pagination/sort/search policy,
      `WriteOp` shape, cache surface, notification demux
- [ ] Create `vantage-coop` crate
- [ ] Define `Coop` / `CoopBuilder` / `CoopShell`
- [ ] `with_pagination()` — slice over inner `list_vista_values`;
      respect `Vista::pagination()` from stage 5b
- [ ] `with_sort()` — sort inner result against
      `Vista::orders()` (also stage 5b)
- [ ] `with_search()` — case-insensitive substring across columns
      flagged `SEARCHABLE`; respects `Vista::add_search` state from
      stage 5b
- [ ] Define `Cache` trait + `MemCache` impl
- [ ] `with_cache(cache)` — read-through; cache key from `{driver,
      table, condition hash, paginate hash, sort hash, search hash}`;
      invalidation hooks fire on every write
- [ ] Define `WriteOp` enum + handler trait
- [ ] `with_writes(handler)` — dispatches every `insert/replace/patch/
      delete/insert_return_id` call to the handler; reports
      `can_insert: true` etc. regardless of inner caps
- [ ] Define `LiveStream` trait (port from vantage-live, or replace)
- [ ] `with_live(stream)` — consumes notifications, invalidates the
      cache (broad-by-table for v1)
- [ ] vantage-surrealdb wires LIVE queries into a `LiveStream` impl
      (closes `../../TODO.md` "Wire up real LIVE query support
      end-to-end")
- [ ] `CoopFactory` convenience for auto-Coop'd Vistas from a factory
- [ ] Capability transformation rules documented + tested
- [ ] Composition test — Coop'ing a Coop'd Vista works and the outer
      capability set is the union of fillers
- [ ] Integration test: read-only CSV + `with_writes(audit_log)` →
      writes succeed and land in the audit log
- [ ] Integration test: REST source without native pagination +
      `with_pagination()` → `Vista::set_pagination(Some(...))` slices
      correctly
- [ ] Integration test: SurrealDB read + `with_cache + with_live` →
      cache invalidates from a real LIVE event

## References

- Subsumes:
  - `../../TODO.md` "Wire up real LIVE query support end-to-end" — the
    full sub-tree (surreal-client, vantage-surrealdb, vantage-live demo,
    helper script, CHANGELOG entries)
  - `../../FINAL_TODO.md` "RouterDataSet" — `with_writes` is the
    generalised form
  - `../../FINAL_TODO.md` "save_into(other_table)" — naturally
    expressible as a `with_writes` handler that forwards to a second
    Vista
- Touches:
  - `../../FINAL_TODO.md` "Caching / domain-specific extensions" docs
    items — narrative for docs once delivered
  - `../../FINAL_TODO.md` "Closure-based bulk update" — `with_writes`
    handler gets visibility into every mutation, can batch upstream
- Pairs with:
  - Stage 5b (query controls): every method that returns `Unsupported`
    on a driver becomes a Coop fill-in target. The two stages define
    the same surface from opposite sides — 5b adds the API, 7 adds the
    fallback implementations.
- Closes (once delivered):
  - The legacy `vantage-live` crate's role; LiveStream and Cache live
    in vantage-coop
