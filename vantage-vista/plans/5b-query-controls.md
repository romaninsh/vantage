# Stage 5b — Query controls (sort, pagination, search, aggregates)

Status: **Not started**

Vista today exposes schema, eq-narrowing, and `get_count`. Everything else
a consumer would want to do to a result set — sort it, paginate it, search
across `SEARCHABLE`-flagged columns, ask for a sum/max/min — has no Vista
surface yet. This stage closes that gap.

These are sibling concerns to stage 5 conditions (all "shape the result
set" operations) but kept in a separate stage because the design space is
unrelated: conditions need operator/policy machinery, sort/paginate/search
are simple delegations to the driver, and aggregates are scalar reads.

Architecturally they pair with `vantage-diorama`: every method here is
expected to return `Unsupported` on drivers that can't push it down, and
a Diorama-wrapped Vista fills the gap client-side via its cache and
callbacks. The Vista surface stays uniform regardless.

## Discussion phase

- [ ] Should sort live on Vista as flat `add_order(field, direction)` or
      mirror Table's full `OrderBy<E>` shape? Lean: flat, since Vista
      doesn't expose `T::Condition`-style expressions to callers anyway.
- [ ] `Vista::add_order` value type — `&str` field name, or accept
      computed expressions? Lean: field name only for v1; computed-order
      follows once `with_expression` lands (could move to stage 6 hooks).
- [ ] Removable sort handles (mirroring `temp_add_order` on Table)?
      Lean: yes, same handle pattern as conditions in stage 5.
- [ ] `set_pagination` carries the existing `vantage_table::Pagination`
      struct verbatim, or does Vista get its own `VistaPagination`
      mirroring `PaginateKind`? Lean: reuse `Pagination`; the
      `PaginateKind` capability tells UIs which controls to render but
      doesn't gate the setter.
- [ ] Search method signature — `add_search(&str)` that auto-builds an OR
      across `SEARCHABLE` columns, or `Vista::search_expression(&str)`
      that returns the condition for the caller to apply? Lean:
      `add_search` (mutates), matching `Table::add_search`. Drivers that
      can push it down (SQL `LIKE`, Mongo `$regex`) do so; others
      return `Unsupported` and a Diorama-backed Vista fetches and filters
      in memory using its cache.
- [ ] Aggregates `get_sum/max/min` value type — `CborValue` (universal)
      or driver-typed? Lean: `CborValue` for consistency with
      `get_count`'s `i64`-as-CBOR-int and the rest of the Vista
      boundary.
- [ ] Aggregate column reference — by name (string) or by
      `&Column` lookup result? Lean: name only; Vista doesn't expose a
      typed-column path.
- [ ] Computed/expression columns (`Table::with_expression` parity) —
      Vista-native or stage 6 hooks? Lean: defer to hooks; a stage-6
      `after_select` hook can synthesise computed fields and is more
      flexible than mirroring the Rust closure form.

## Scope

In:

- `Vista::set_pagination(Option<Pagination>)` / `Vista::pagination()`
  pair; `TableShell::set_pagination` / `get_pagination` driver hooks
- `Vista::add_order(field, SortDirection)` / `temp_add_order` /
  `temp_remove_order` w/ `OrderHandle`; `TableShell::add_order` driver hook
- `Vista::add_search(value)` method backed by the `SEARCHABLE` flag
  vocabulary; `TableShell::add_search` driver hook (default
  `Unsupported`)
- `Vista::get_sum(field) / get_max(field) / get_min(field)` returning
  `Result<CborValue>`; `TableShell::get_vista_sum/max/min` driver hooks
- New capability flags: `can_paginate_native`, `can_sort_native`,
  `can_search_native`, `can_aggregate_native` (advise consumers when
  push-down is real vs. Diorama-filled). Open question: do we collapse the
  existing `paginate_kind` into the same vocabulary?
- Aggregate semantics on a conditioned Vista — `get_sum` respects the
  current condition set, same as `get_count`. Document this.

Out:

- Computed/expression columns (`with_expression`) — deferred to stage 6
  hooks
- GROUP BY / HAVING — needs a different mental model (multi-row
  reduction); follow-up after stage 9
- UNION / EXCEPT — same; covered by `FINAL_TODO.md` table-level union
  item
- Cross-column / cross-reference search (e.g. `clients.name LIKE` AND
  `orders.total > 0`) — out of scope; would need the traversal-aware
  condition machinery from stage 6/7
- `get_avg` — not in the legacy Table surface; add if a real use case
  shows up

## Plan

- [ ] Discuss with user: sort signature, search semantics, aggregate
      return type, capability-flag vocabulary
- [ ] Add `Vista::set_pagination` / `pagination()` accessor pair
- [ ] Extend `TableShell` with `set_pagination` / `get_pagination`;
      in-tree drivers (CSV, MongoDB) override with real impls
- [ ] Add `Vista::add_order` / temp variants with `OrderHandle`; extend
      `TableShell::add_order` (default `Unsupported`); CSV + Mongo
      override
- [ ] Add `Vista::add_search(value)`; extend `TableShell::add_search`
      (default `Unsupported`); Mongo overrides via `$regex` across
      SEARCHABLE fields; CSV overrides via column scan; SQL drivers
      compose `field LIKE` ORs when stage 5 lands
- [ ] Add `Vista::get_sum / get_max / get_min` with `CborValue` return;
      extend `TableShell` with matching `get_vista_sum/max/min` (default
      `Unsupported`); CSV + Mongo override
- [ ] Decide capability-flag vocabulary; update `VistaCapabilities`
- [ ] Integration tests: each new method on CSV + Mongo, plus a
      `Unsupported` assertion on a stub driver that opts out
- [ ] Cross-link to `vantage-diorama`: every `Unsupported` path here
      is a Diorama fill-in target; confirm Diorama's callback surface
      matches what this stage defines

## References

- Subsumes:
  - `../../FINAL_TODO.md` "Search across all columns" (legacy
    `add_search` parity) — closed once `Vista::add_search` lands
- Touches:
  - `../../FINAL_TODO.md` "Closure-based bulk update" — orthogonal but
    sort+paginate make iterative client-side bulk updates ergonomic;
    revisit after this lands
  - `../../TODO.md` "Move `get_count`/`get_sum`/`get_max`/`get_min` off
    `SelectableDataSource`" — Vista-level aggregates need the trait
    boundary fix to land first (or work around via per-driver shell
    impls)
- Pairs with:
  - Stage 5 (conditions): same delegation pattern, separate concern
  - `vantage-diorama`: provides the client-side fallback for every
    method here that a driver can't push down
