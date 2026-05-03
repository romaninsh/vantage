# Stage 5 — Portable conditions + per-column policy

Status: **Not started**

Add portable condition support: callers add conditions through the
universal Vista API in CBOR currency; drivers translate to native form;
per-column policy gates which operators are allowed. Closes the largest
vantage-ui pain point (master/detail filtering only works on AWS today).

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

## Scope

In:

- `Op` enum (universal operator vocabulary)
- Per-column condition policy (runtime metadata)
- `Vista::add_condition(column, op, value) -> Result<ConditionHandle>`
- `Vista::remove_condition(handle)`
- Driver-side `VistaSource::translate_condition(...)` for each in-tree
  driver
- Default type-driven policy in `Column`
- YAML-time policy override per column

Out:

- Nested AND/OR composition (v2)
- Search-as-condition (separate concept, may share policy machinery
  later)
- Hook-mediated condition rewriting (stage 6)

## Plan

- [ ] Discuss with user: all open questions above
- [ ] Define `Op` enum
- [ ] Define `ConditionPolicy` per column (set of allowed ops)
- [ ] Default-policy table by column type
- [ ] YAML schema extension: per-column `conditions: [eq, like, ...]`
      override
- [ ] `Vista::add_condition` / `remove_condition` with handle
- [ ] Implement `translate_condition` for each driver:
      sqlite/mongo/surreal/aws/rest/csv
- [ ] REST per-column server-capability declaration in `rest:` extras
- [ ] Integration test: master/detail traversal works on
      sqlite/surreal/mongo/rest (closes the AWS-only asymmetry)

## References

- Subsumes:
  - `/Users/rw/Work/vantage-ui/app/todo/anytable-portable-conditions.md`
    — closes this issue
  - `../../TODO.md` "Decouple `column_table_values_expr` from
    `ExprDataSource`" — driver translation owns this concern
  - `../../FINAL_TODO.md` "Nested condition composition" — partially;
    flat AND only here, nested deferred
- Touches:
  - `../../TODO.md` "Condition::or() shouldn't be limited to only two
    arguments" — when nested composition lands, this collapses
  - `../../TODO.md` "Explore Selectable parameterized on condition type"
    — informs how SQL drivers route
