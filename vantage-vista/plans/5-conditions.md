# Stage 5 — Portable conditions + per-column policy

Status: **Partial** — driver-typed `eq` delegation shipped alongside stage
4. Remaining work: extend the operator vocabulary beyond `eq`, add
per-column policy, and bring the SurrealDB / AWS / REST drivers along
with the rest of stage 4.

## What already shipped (stage 4 spillover)

The original plan deferred *all* condition translation to this stage. In
practice, the moment a second driver landed it became obvious that
in-memory filtering was the wrong baseline, so the equality case got
moved up:

- `Vista::add_condition_eq(field, CborValue) -> Result<()>` — universal
  entry point; delegates to the source.
- `VistaSource::add_eq_condition(&mut self, field, value)` — driver
  contract; default impl returns `Unimplemented`. CSV translates to
  `Expression<AnyCsvType>`; Mongo translates to `bson::Document` and
  honours `column_paths` for nested fields (dot notation server-side).
- Vista carries no condition state — every call mutates the wrapped
  `Table`, which means push-down is automatic wherever the backend
  supports it.

Stage 5 is now scoped to *the rest*: the operator vocabulary, per-column
policy, removal/handles, and the operator translations for the remaining
drivers.

## Discussion phase

This is the deferred deep-dive (Q1 from earlier discussion).

- [ ] Per-column policy: compile-time on typed Table (typestate),
      runtime metadata only, or both? Lean: runtime metadata only.
- [ ] Operator vocabulary: fixed `Op` enum vs extensible registry?
      Lean: fixed enum (`Eq`, `Ne`, `Lt`, `Lte`, `Gt`, `Gte`, `Like`,
      `In`, `IsNull`, `IsNotNull` — small, exhaustive, can grow).
- [ ] Default policy when YAML doesn't declare: type-driven defaults
      (Int → `Eq | Lt | Gt | In`, String → `Eq | Like | In`, …)?
- [ ] Composition: AND/OR trees, or only flat AND-of-conditions for v1?
      Lean: flat AND-of-conditions for v1, document path to nested.
- [ ] Failure mode for unsupported op on a column: hard error vs warning
      vs silent drop. Lean: hard error at the `add_condition` call site.
- [ ] REST translation: how does a REST driver declare per-column server
      capabilities? Per-column extras block? A condition map in
      `rest:` extras?
- [ ] Removal: handle-based (`ConditionHandle` → `remove_condition`) or
      a clear-all/replace-all model? Lean: handles, mirroring the
      table's existing `temp_add_condition` pattern.
- [ ] How does the new `add_condition(op, value)` coexist with the
      already-shipped `add_condition_eq`? Lean: keep `add_condition_eq`
      as a thin wrapper for the common case; the general entry point
      uses the `Op` enum.

## Scope

In:

- `Op` enum (universal operator vocabulary)
- Per-column condition policy (runtime metadata)
- `Vista::add_condition(field, op, value) -> Result<ConditionHandle>`
- `Vista::remove_condition(handle)`
- Driver-side `VistaSource::add_condition(field, op, value)` extending
  the existing `add_eq_condition` baseline; in-tree drivers translate
  to their native condition type
- Default type-driven policy in `Column`
- YAML-time policy override per column

Out:

- Nested AND/OR composition (v2)
- Search-as-condition (separate concept, may share policy machinery
  later)
- Hook-mediated condition rewriting (stage 6)
- Re-engineering the already-shipped `add_eq_condition` — it stays as
  the public sugar for the common case, internally routed through the
  new `add_condition` once it exists

## Plan

- [ ] Discuss with user: open questions above
- [ ] Define `Op` enum
- [ ] Define `ConditionPolicy` per column (set of allowed ops)
- [ ] Default-policy table by column type
- [ ] YAML schema extension: per-column `conditions: [eq, like, ...]`
      override
- [ ] Add `Vista::add_condition` / `remove_condition` with handle;
      reroute `add_condition_eq` to call through it
- [ ] Add `VistaSource::add_condition` trait method (default
      `Unimplemented`); update CSV + Mongo to override
- [ ] Implement remaining drivers' eq + full operator translations:
      sqlite/surreal/aws/rest (coordinate with stage 4 driver rollout)
- [ ] REST per-column server-capability declaration in `rest:` extras
- [ ] Integration test: master/detail traversal works on
      sqlite/surreal/mongo/rest (closes the AWS-only asymmetry)

## References

- Subsumes:
  - `/Users/rw/Work/vantage-ui/app/todo/anytable-portable-conditions.md`
    — closes this issue once the operator vocabulary lands
  - `../../TODO.md` "Decouple `column_table_values_expr` from
    `ExprDataSource`" — driver translation owns this concern
  - `../../FINAL_TODO.md` "Nested condition composition" — partially;
    flat AND only here, nested deferred
- Touches:
  - `../../TODO.md` "Condition::or() shouldn't be limited to only two
    arguments" — when nested composition lands, this collapses
  - `../../TODO.md` "Explore Selectable parameterized on condition type"
    — informs how SQL drivers route
