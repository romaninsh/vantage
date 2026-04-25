# Changelog

## 0.4.4 — 2026-04-25

- **Breaking**: `AnyTable` now carries `ciborium::Value` instead of `serde_json::Value`. Backends already store CBOR (surreal/sql) or BSON (mongo) internally; this drops the last lossy JSON hop at the type-erased boundary.
- Renames the internal `JsonAdapter` to `CborAdapter`. Constructor signatures (`AnyTable::new` / `AnyTable::from_table`) unchanged at call sites.
- New `CborValueExt` trait re-exported from `prelude` — adds `as_str()`, `as_i64()`, `as_u64()`, `as_f64()`, `get(&str)`, `get_mut(&str)` so consumer code stays one-liner-shaped on top of `Record<ciborium::Value>`.
- Migration: AnyTable consumers that matched on JSON variants need to either match on `ciborium::Value` arms or call `serde_json::to_value(&record)?` at the consumer edge.
