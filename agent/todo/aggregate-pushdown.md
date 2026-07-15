# Aggregate pushdown: server-side sum/count behind a capability

**Today:** `ValueScenery` computes every aggregate — `count`, `sum`, `max`,
`min`, `custom` — by scanning the Dio cache, unconditionally. There is no
aggregate method on `TableShell` and no capability to advertise one;
`VistaCapabilities::can_count` exists but even `ValueScenery::count()`
ignores it. The local scan is *coverage-honest*: it aggregates what has been
observed so far and climbs as hydration proceeds (learn-6's `total rows`
status value depends on exactly this).

**Want:** a `can_aggregate` capability + a `TableShell` aggregate method, so
backends that can compute server-side (SQL, Surreal, some REST reports) do.
Routing rules:

- **Augment-owned columns always compute locally** — they only exist in the
  cache (learn-6's `rows` is derived from file contents; the master has never
  heard of it). Pushdown is impossible in principle.
- **Native columns may push down** when the capability is there — but the
  result is the *authoritative total*, not the observed one, and it must
  re-query on `Refreshing`/`DatasetChanged` instead of recomputing from local
  state.

**Do NOT switch silently.** Observed-vs-authoritative is a semantic choice,
not an optimization: the same `sum(col)` would mean "total of everything" on
one backend and "total of what we've seen" on another. Make it visible in the
API — e.g. `sum(col)` (authoritative when capable, error/local otherwise) vs
`sum_observed(col)` (always local, coverage-honest) — and document which one
a status bar should use during hydration.

**Consumers:** ValueScenery builders, learn-6/learn-7 status values, any
vantage-ui aggregate badge.
