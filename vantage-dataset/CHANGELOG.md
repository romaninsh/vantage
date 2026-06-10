# Changelog

## 0.6.0 — 2026-06-10

- `ImTable` now supports mutability: `insert`, `update`, and `delete` operations through
  `WritableDataSet`/`WritableValueSet` trait implementations, replacing the previous read-only
  clone-on-every-op model.
- `ImDataSource` read-modify-write race fixed — concurrent updates are no longer silently lost.
- Eliminated full-table clone on every operation; writes now acquire a write lock and apply changes
  in-place.
- Added `DatasetInsertable`, `DatasetReadable`, `DatasetWritable`, `ValuesetInsertable`,
  `ValuesetReadable`, `ValuesetWritable` trait impls for `ImTable`.

## 0.5.2 — 2026-05-31

- `ImTable::seed` synchronously replaces a table's rows from a known collection, without the async
  insert path — backing the in-memory sub-Vistas used by contained relations.

## 0.5.1 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.5.0 — 2026-05-23

- [`ImDataSource`](https://docs.rs/vantage-dataset/0.5.0/vantage_dataset/im/struct.ImDataSource.html)
  and [`ImTable`](https://docs.rs/vantage-dataset/0.5.0/vantage_dataset/im/struct.ImTable.html) are
  now generic over the wire value type `V`. The default stays `serde_json::Value`, so existing call
  sites compile unchanged. The entity-typed
  [`ReadableDataSet`](https://docs.rs/vantage-dataset/0.5.0/vantage_dataset/traits/trait.ReadableDataSet.html)
  / `WritableDataSet` / `InsertableDataSet` impls are only available for `V = serde_json::Value`
  because they round-trip records through `serde_json` for `try_from_record`; the value-typed
  valueset impls now work for any `V` with `Clone + Send + Sync + 'static`. Drivers that want a
  CBOR-typed in-memory store can use `ImDataSource::<CborValue>::new()` directly without bringing
  serde_json into their value path.
- Bumped to the 0.5 line to track the workspace's `AnyTable` decommission cycle.

## 0.4.3 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.4.2 — 2026-04-19

- `Operation::is_null()` / `is_not_null()` on the generic trait — SQL backends render `{} IS NULL` /
  `{} IS NOT NULL`; Mongo gets `{ field: null }` / `{ field: { $ne: null } }`.
- `ActiveEntity::reload()` — refetches by stored id; errors if the row was deleted externally.
- `ActiveEntity::delete()` — deletes by stored id.
- `ReadableDataSet::get(id)` and `ReadableValueSet::get_value` now return `Result<Option<E>>` /
  `Result<Option<Record>>` instead of `Err("no row found")` on miss.
