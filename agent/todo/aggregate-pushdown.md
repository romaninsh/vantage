# Aggregate pushdown: server-side sum/count behind a capability

**Status: scaffolding landed, nothing uses it.** The transport exists on
`main` and no backend implements it, so every aggregate still resolves
by scanning the Dio cache. Audited 2026-08-22 — the "Today" section below
described the state *before* that scaffolding merged and was stale.

## What is on main

- `vantage-vista/src/aggregate.rs` — `AggregateSpec { op, column, alias,
  group_by }` with collision-free `cache_key` derivation, unit-tested.
- `TableShell::aggregate_vista(&self, vista, spec) -> Result<Option<Vista>>`
  (`vantage-vista/src/source.rs:240`) — the pushdown hook. Documented at
  length: the returned vista's capabilities describe the DERIVED set, and
  must not inherit condition support, because a condition on an aggregate
  is `HAVING` — a different operation over different values.
- `Vista::aggregate` (`vantage-vista/src/vista.rs:204`) and
  `Dio::master_aggregate` (`vantage-diorama/src/dio/mod.rs:711`).

An aggregation deliberately returns a **new set**, not a number: `count(*)`
is a one-row table, `GROUP BY` is one row per group, and both are ordinary
sets you can condition, order or count again. That shape decision is
settled and worth keeping.

## What is missing

1. **No implementors.** `aggregate_vista` is overridden by exactly zero
   drivers — SQL, Surreal, Mongo, REST all inherit the default `Ok(None)`.
   Every call takes the local path. Compare `add_op_condition`, whose
   equivalent rollout *did* reach six backends.
2. **No `can_aggregate` capability.** `VistaCapabilities` carries 17
   `can_*` flags and none for aggregation, so a consumer cannot ask
   whether pushdown is available before requesting it — it can only call
   and interpret `None`.
3. **`ValueScenery` never attempts pushdown.** It still reduces over the
   cache unconditionally via its own `Aggregate` enum
   (`Count`/`CountWhere`/`Sum`/`Max`/`Min`/`Custom`,
   `vantage-diorama/src/scenery/value.rs:79`). `master_aggregate` and
   `ValueScenery` are two disconnected paths; nothing bridges them.
4. **No observed-vs-authoritative split.** Neither `sum_observed` nor any
   equivalent exists. This is the requirement the original note called
   out hardest and it is entirely unstarted — see below.
5. **A rotted doc example.** `Dio::master_aggregate`'s doc comment builds
   its spec with `AggregateSpec::new("count", "failed").condition("Event", …)`.
   There is no `condition` method — the builder is `new`/`column`/`group_by`
   only, and `AggregateSpec`'s own docs say conditions deliberately do not
   live there (narrow the source vista first, then aggregate it). The
   example is ```` ```ignore ```` so nothing catches it.

## The semantic decision, still open

The local scan is *coverage-honest*: it aggregates what has been observed
so far and climbs as hydration proceeds. learn-6's `total rows` status
value depends on exactly that behaviour.

Pushing down changes the meaning. The same `sum(col)` would be "total of
everything" on one backend and "total of what we've seen" on another.

**Do not switch silently.** Make it visible in the API — e.g. `sum(col)`
(authoritative when capable, error otherwise) vs `sum_observed(col)`
(always local) — and document which one a status bar should use during
hydration.

Routing rules, once there is something to route:

- **Augment-owned columns always compute locally.** They only exist in the
  cache (learn-6's `rows` is derived from file contents; the master has
  never heard of it). Pushdown is impossible in principle.
- **Native columns may push down** when the capability is there — but the
  result is the authoritative total, and it must re-query on
  `Refreshing`/`DatasetChanged` rather than recomputing from local state.

**Consumers:** ValueScenery builders, learn-6/learn-7 status values, any
vantage-ui aggregate badge.
