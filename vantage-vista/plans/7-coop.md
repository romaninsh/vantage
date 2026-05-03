# Stage 7 — vantage-coop crate

Status: **Not started**

Separate crate. Coop wraps a `VistaFactory` and adds caching, write
routing, and live-event invalidation. Coop is invisible to consumers —
once a Vista is built through a Coop, nothing in the consumer's surface
changes; only runtime behaviour does.

## Discussion phase

Items intentionally deferred from earlier rounds — re-open here.

- [ ] Notification demux strategy — single per-connection stream that
      Coop fans out by table name, vs per-Vista subscription?
- [ ] `with_upstream` API — factory of writers vs single Vista vs closure
      `Fn(&VistaSpec) -> Result<Vista>`?
- [ ] Vista lifetime vs factory lifetime — Vistas self-contained
      (sources own their resources via `Arc`) so factory can drop?
      Notifications are a shared resource — how is that ownership
      modelled?
- [ ] Cache key strategy — auto from `{datasource}/{name}` (default) +
      manual override?
- [ ] Cache abstraction surface — minimum: `get`, `put`,
      `invalidate_prefix`, `invalidate_id`?
- [ ] Backends to ship with: in-memory only for v1? Filesystem?
      Pluggable trait so users can plug Redis themselves?
- [ ] Capability transformation: Coop with `with_upstream(write_log)`
      flips `can_write: false` → `can_write: true` — confirm semantics
      and how the upstream is invoked

## Scope

In:

- New `vantage-coop` crate
- `Coop::new(factory)` — wraps an inner factory; itself implements
  `VistaFactory`
- `with_cache(cache)`, `with_upstream(...)`, `with_notifications(stream)`
- `Cache` trait and at least one impl (in-memory)
- `LiveStream` trait — re-housed here from current vantage-live, or
  re-implemented
- Capability transformation logic (Coop adjusts capabilities reported
  to consumers based on which knobs are configured)
- SurrealDB LIVE-query wiring — subscribed via vantage-surrealdb's
  notification pipe, fed to Coop's invalidation channel

Out:

- Distributed caches (Redis, etc.) — pluggable trait left for users
- Cross-Vista cache coherency
- Write-ahead-log persistence

## Plan

- [ ] Discuss with user: notification demux, upstream API, lifetimes,
      cache key strategy, cache abstraction
- [ ] Create `vantage-coop` crate
- [ ] Define `Coop<F: VistaFactory>` struct
- [ ] Implement `VistaFactory for Coop<F>` — produces Vistas whose
      source is the Coop layer wrapping the inner source
- [ ] Define `Cache` trait (`get`, `put`, `invalidate_id`,
      `invalidate_prefix`)
- [ ] In-memory `MemCache` impl
- [ ] Define `LiveStream` trait (port from vantage-live or replace)
- [ ] Notification demux machinery
- [ ] Capability transformation rules documented + tested
- [ ] vantage-surrealdb wires LIVE queries into a `LiveStream` impl
      (closes `../../TODO.md` "Wire up real LIVE query support
      end-to-end")
- [ ] Integration test: read-through cache + invalidation
- [ ] Integration test: write-routing via `with_upstream`
- [ ] Integration test: capability re-addition (CSV `can_write: false`
      + Coop upstream → `can_write: true`)

## References

- Subsumes:
  - `../../TODO.md` "Wire up real LIVE query support end-to-end" — the
    full sub-tree (surreal-client, vantage-surrealdb, vantage-live demo,
    helper script, CHANGELOG entries)
  - `../../FINAL_TODO.md` "RouterDataSet" — `with_upstream` is the
    generalised form
- Touches:
  - `../../FINAL_TODO.md` "Caching / domain-specific extensions" docs
    items — narrative for docs once delivered
- Closes (once delivered):
  - The legacy `vantage-live` crate's role; LiveStream and Cache live
    in vantage-coop
