# Stage 6 — Hooks lifecycle + Rhai integration

Status: **Not started**

Add hook support: pre/post lifecycle callbacks for read, insert, update,
delete. Hooks declared in YAML run as Rhai scripts; hooks added in Rust
code use a typed trait. vantage-ui consumes hooks for audit, validation,
soft-delete, and similar cross-cutting concerns.

## Discussion phase

Deferred deep-dive (Q2 from earlier).

- [ ] Read-only observers vs mutating interceptors vs both?
      Lean: both, distinguished at the trait. `Observer::observe(&Ctx)`,
      `Interceptor::intercept(&mut Ctx) -> Outcome`.
- [ ] Hook outcome enum — `Continue`, `Skip`, `Reject(error)`?
- [ ] Lifecycle points: `before_select`, `after_select`, `before_insert`,
      `after_insert`, `before_update`, `after_update`, `before_delete`,
      `after_delete` — confirm full set
- [ ] Hook ordering when multiple hooks are registered — registration
      order? priority field?
- [ ] Rhai context bindings: which Vista APIs are exposed to scripts
      (record manipulation, condition addition, reject, log)?
- [ ] Where hooks live on the Vista — typed `HookCollection` field, or a
      separate `HookRegistry` injected by the factory?
- [ ] Programmatic Rust hook registration — can hooks be added to a
      Vista *after* construction (vantage-ui need)?

## Scope

In:

- `Hook` trait (or split `Observer` / `Interceptor`)
- `HookCtx` struct with read/write access depending on lifecycle
- `Outcome` enum
- YAML `hooks:` block parsed into Rhai scripts
- Rhai engine + Vista context bindings
- Programmatic `Vista::with_hook(...)` Rust-side
- Integration test: a Rhai `before_insert` script that rejects a record

Out:

- Backend-specific query rewriting via hooks (lazy expressions —
  `BeforeQuery` / `AfterQuery` from FINAL_TODO) — listed but not
  implemented here; possibly stage 6.5
- Soft-delete extension (use the new hook surface; not implemented in
  this stage)

## Plan

- [ ] Discuss with user: hook signature, outcome semantics, lifecycle set,
      ordering
- [ ] Define `Hook` trait(s) and `Outcome` enum
- [ ] Define `HookCtx` per lifecycle
- [ ] Add Rhai dependency to `vantage-vista` (hard dep, no feature flag)
- [ ] Bind Vista record / condition / reject APIs into Rhai engine
- [ ] YAML `hooks:` block parser; compile to Rhai AST at construction
      time, store on Vista
- [ ] `Vista::with_hook(...)` for Rust-side registration
- [ ] Hook execution wired into TableShell CRUD calls (probably in
      `vantage-vista` not the driver — driver delegates)
- [ ] Integration test with a Rhai validation hook
- [ ] Document Rhai context API in `HOOKS.md` next to this plan

## References

- Subsumes:
  - `../../PLAN_0_5.md` §3 "Table-level hooks" — fully replaces
  - `../../FINAL_TODO.md` "Hooks / extensions framework" — fully
    replaces; legacy `TableExtension` retired by this surface
- Touches:
  - `../../FINAL_TODO.md` "Lazy expressions / post-fetch transforms" —
    `AfterQuery` variant maps to an `after_select` hook; `BeforeQuery`
    variant deferred (driver-specific query construction)
- Closes (once delivered):
  - PLAN_0_5 §3 hook trait + outcome enum
  - SoftDelete reference impl can land in a follow-up using this
    surface
