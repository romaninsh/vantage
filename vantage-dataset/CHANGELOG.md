# Changelog

## 0.4.2 — 2026-04-19

- `Operation::is_null()` / `is_not_null()` on the generic trait — SQL backends render `{} IS NULL` / `{} IS NOT NULL`; Mongo gets `{ field: null }` / `{ field: { $ne: null } }`.
- `ActiveEntity::reload()` — refetches by stored id; errors if the row was deleted externally.
- `ActiveEntity::delete()` — deletes by stored id.
- `ReadableDataSet::get(id)` and `ReadableValueSet::get_value` now return `Result<Option<E>>` / `Result<Option<Record>>` instead of `Err("no row found")` on miss.
