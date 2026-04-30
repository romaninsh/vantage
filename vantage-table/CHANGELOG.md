# Changelog

## 0.4.8 — 2026-04-30

- `TableLike` gains five reflection / mutation methods so type-erased callers (the new model-driven CLI in `vantage-cli-util`, anything else holding an `AnyTable`) can reach metadata that previously only existed on the typed side. All have default impls so existing implementors compile unchanged.
  - `id_field_name() -> Option<String>`
  - `title_field_names() -> Vec<String>`
  - `column_types() -> IndexMap<String, &'static str>`
  - `get_ref_names() -> Vec<String>`
  - `add_condition_eq(field, value) -> Result<()>`
- New `TableSource::eq_condition(field, value) -> Result<Self::Condition>` (default: error). Backends that support textual eq filtering override; `add_condition_eq` dispatches through it.
- New `Table::with_title_column_of::<Type>(name)` builder — adds a typed column and records its name in an ordered `title_fields: Vec<String>` on the table. `title_field_names()` returns that vec, then unions in any columns that carry `ColumnFlag::TitleField` directly.
- `AnyTable` and the internal `CborAdapter` forward all five new methods through to the wrapped table.

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
