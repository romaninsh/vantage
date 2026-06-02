# SurrealDB Rhai DSL Engine

## Context

Implement a Rhai scripting engine for SurrealDB, analogous to the SQL Rhai engine in
`vantage-sql/src/rhai_engine/`. Self-contained module in `vantage-surrealdb` — no dependency on
vantage-sql's rhai_engine. Enables building SurrealDB queries via `.rhai` scripts.

## Steps

- [ ] **Step 1** — Add `rhai` as optional dependency in `vantage-surrealdb/Cargo.toml` with a `rhai`
      feature flag
- [ ] **Step 2** — Create `src/rhai_engine/` module: wrapper types (`RhaiIdent`, `RhaiExpr`,
      `RhaiSelect`), conversion helpers, comparison operators, constructors (`ident`, `table`,
      `expr`, `fx`, `thing`), aggregates (`count`, `sum`/`math::sum`, `math::max`, `math::min`),
      select methods (`from`, `field`, `expression`, `where`, `order_by`, `group_by`, `limit`,
      `only`, `value`, `distinct`), and the `register_surreal_engine!` macro
- [ ] **Step 3** — Add SurrealDB-specific features: graph traversal (`.arrow()`, `.back()`),
      `parent()` for `$parent`, `type::thing()` constructor, and SurrealDB function wrappers
- [ ] **Step 4** — Create test infrastructure: `.rhai` → `.surql` golden file test runner (as
      example binary) and initial smoke test scripts
- [ ] **Step 5** — Run `cargo check` and verify golden files match expected SurrealQL output

## Decisions

- Self-contained in vantage-surrealdb (no shared crate) — wrapper types are ~40 lines each, not
  worth cross-crate overhead
- SurrealDB has no JOINs — skip `inner_join`/`left_join`, use graph traversal instead
- `LIMIT n START s` instead of `LIMIT n OFFSET s`
- Functions use SurrealDB namespacing: `count()`, `math::sum()`, `math::max()`, `math::min()`,
  `array::group()`

## Open questions

- (empty — ready to execute)

## Out of scope

- INSERT/UPDATE/DELETE Rhai builders (future)
- LET variable bindings (future)
- RELATE/graph-edge creation (future)
- Live queries (future)
- Window functions (SurrealDB doesn't have them)
