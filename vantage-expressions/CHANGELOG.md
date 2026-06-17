# Changelog

## 0.6.1 — unreleased

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
