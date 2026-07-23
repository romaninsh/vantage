# Changelog

## 0.6.2 — 2026-07-23

- `VantageError::message()` and `location()` accessors: the bare message
  and capture site, without the context map or source chain — for UI
  surfaces that render those parts separately (the context map is
  already public). `Display` remains the combined one-line form.

## 0.6.1 — 2026-07-04

- `VantageError::traced_debug()`: emit the same structured event as
  `traced()` at DEBUG level — for errors that are a legitimate answer to a
  probing caller (e.g. an `Unsupported` capability refusal) rather than
  faults.

## 0.6.0 — unreleased

- **Breaking:** the error-kind annotators `VantageError::is_unsupported` /
  `is_unimplemented` / `is_incorrect_usage` are renamed to `mark_unsupported` /
  `mark_unimplemented` / `mark_incorrect_usage`. The `is_*` names now denote
  real `&self -> bool` predicates. Trace emission is decoupled from
  classification: `mark_*` no longer logs implicitly — chain the new
  `.traced()` to emit the `tracing::error!` event
  (e.g. `error!(...).mark_unsupported().traced()`).

## 0.5.0 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.4.2 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.4.1 — 2026-05-04

- New error-kind annotators:
  [`VantageError::is_unimplemented`](https://docs.rs/vantage-core/0.4.1/vantage_core/util/error/struct.VantageError.html)
  and
  [`is_unsupported`](https://docs.rs/vantage-core/0.4.1/vantage_core/util/error/struct.VantageError.html).
  Use them on errors returned from default trait method impls so callers can distinguish "the driver
  doesn't support this op" from "the driver claims support but the implementation is missing."

## 0.4.0 — 2026-04-16

- Initial 0.4 release. `VantageError` with structured context, `Result` alias, and the unified
  error-handling foundation used by every other vantage crate.
