# Changelog

## 0.1.2 — 2026-07-22

- Depend on vantage-diorama 0.7 (WriteOp retired for ChangeFlash; no
  adapter-side behavior change).

## 0.1.1 — 2026-07-16

- `DioRouter::key_by` now accepts non-string ids. The identity-watch diff keyed a
  row only when its id field serialized to a JSON string, silently skipping every
  row otherwise — so a watch keyed on a SurrealDB `Thing` (a tagged object) or a
  numeric id emitted nothing. It now derives a stable key from any JSON value, so
  `key_by` works across backends (string, numeric, Mongo `ObjectId`, Surreal
  `Thing`). String ids are unchanged.
