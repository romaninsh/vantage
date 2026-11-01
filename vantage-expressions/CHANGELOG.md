# Changelog

## 2.0.0 — 2026-11-01

- Depends on vantage-core 2.0 (no change of its own).

## 1.2.3 — 2026-10-26

- Tests for `Order::suffix` and the `Selectable` builder, inspection and aggregate methods, driven
  through `MockSelect`. No API or behaviour change.

## 1.2.2 — 2026-10-26

- Tests of one behaviour are folded into one test each (`get` converting or failing, macro previews
  filling each placeholder once): 49 tests become 46 with the same coverage. No API or behaviour
  change.

## 1.2.1 — 2026-10-14

- The in-crate unit tests move to the blackbox suite under `tests/`; module docs added. No API or
  behaviour change.

## 1.2.0 — 2026-10-14

- `AnyExpression` and `ExpressionLike` now live here, no longer re-exported from vantage-vista.
  Code that imported them from `vantage_vista` imports them from `vantage_expressions`.
- No longer depends on vantage-vista.

Migration guide: https://vantage-ui.com/book/migration-2.0.html

## 1.1.0 — 2026-10-07

- `execute_by_resolving()`, `execute_strict()` and `defer_execute()`: shared `ExprDataSource` bodies for backends without an expression engine.

## 1.0.1 — 2026-10-04

- `resolve_param()`: collapses a scalar, deferred or nested parameter to one value.
- `Expression::resolve_deferred()`: calls every deferred parameter, nested ones included.
  `MockBuilder` uses it, so it now also resolves deferred parameters inside nested expressions.

## 1.0.0 — 2026-10-03

- Stable release. The API hasn't changed since 2026-06-17 (4 months); no code changes.
- Depends on `vantage-core` 1.0.0, `vantage-dataset` 1.0.0, `vantage-vista` 1.0.0.

## 0.6.1 — 2026-06-17

- Internal dependency realignment for the coordinated 0.6 release; no public API changes.

## 0.6.0 — 2026-06-10

- `IntoValue` for `f64` no longer panics on NaN/Infinity — non-finite floats degrade to
  `Value::Null` instead (JSON has no NaN/Inf representation).
- `Expression::preview()` uses single-pass interleaving instead of repeated `replacen`, so `{}`
  inside a rendered parameter value can no longer corrupt the output.
- Removed the no-op `Flatten::resolve_deferred` method (deferred-parameter resolution is an async
  DataSource concern, not a sync flattener concern).

## 0.5.0 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.4.4 — 2026-05-23

- Internal dependency version refresh; no public API changes.

## 0.4.3 — 2026-05-03

- `AnyExpression` and `ExpressionLike` moved to the new
  [`vantage-vista`](https://docs.rs/vantage-vista/0.4.0/vantage_vista/) crate and are re-exported
  from `vantage-expressions` unchanged. If you import via
  `vantage_expressions::{AnyExpression, ExpressionLike}` (or the prelude), nothing changes for you.

## 0.4.2 — 2026-04-17

- Patch bump in the 0.4 release line.
