# Changelog

## 0.4.7 — 2026-04-29

- New `AnyTable::get_ref(relation) -> Result<AnyTable>`. Lets reference traversal continue on the type-erased side — previously `get_ref` only existed on the typed `Table<T, E>`, so once you wrapped a table into `AnyTable` the relation graph was unreachable.
- `TableLike` gains a `get_ref` method with a default impl that errors out (`"get_ref not supported on this TableLike"`). `Table<T, E>` overrides to delegate to the inherent; `AnyTable` and the internal `CborAdapter` forward to the wrapped table; downstream wrappers like `vantage_live::LiveTable` override to forward through to their master.
- The trait method is sync — `Reference::resolve_as_any` is sync (no IO), so callers don't need an `.await`.

## 0.4.6 — 2026-04-26

- New `AnyTable::from_table_like<T: TableLike<…>>` constructor. Wraps any table-like type that already speaks `Value = CborValue, Id = String` — needed by `vantage-live::LiveTable`, which is `TableLike` but not a `Table<T, E>` instance.

## 0.4.5 — 2026-04-26

- New `ColumnFlag::Indexed` variant. UI hint that a column is cheap to sort or filter on; `vantage-redb` is the only backend that uses it for actual index maintenance.

## 0.4.4 — 2026-04-25

- **Breaking**: `AnyTable` now carries `ciborium::Value` instead of `serde_json::Value`. Backends already store CBOR (surreal/sql) or BSON (mongo) internally; this drops the last lossy JSON hop at the type-erased boundary.
- Renames the internal `JsonAdapter` to `CborAdapter`. Constructor signatures (`AnyTable::new` / `AnyTable::from_table`) unchanged at call sites.
- New `CborValueExt` trait re-exported from `prelude` — adds `as_str()`, `as_i64()`, `as_u64()`, `as_f64()`, `get(&str)`, `get_mut(&str)` so consumer code stays one-liner-shaped on top of `Record<ciborium::Value>`.
- Migration: AnyTable consumers that matched on JSON variants need to either match on `ciborium::Value` arms or call `serde_json::to_value(&record)?` at the consumer edge.
