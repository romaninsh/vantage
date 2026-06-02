# Changelog

## 0.5.3 — 2026-06-02

- Track `vantage-table`'s new `TableSource::Source` associated type (set to `String`; no
  user-visible change).

## 0.5.2 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.5.0 — 2026-05-23

- Bumped to the 0.5 line to track
  [vantage-table 0.5.0](https://docs.rs/vantage-table/0.5.0/vantage_table/)'s opening of the
  `AnyTable` decommission cycle. No code changes beyond the dependency pin.

## 0.4.0 — 2026-04-26

Full rewrite for the 0.4 trait surface. **Storage format and public API are not compatible with
0.3** — open in a fresh database file.

- New `RedbType` type system via `vantage_type_system!` over `ciborium::Value`. `AnyRedbType`
  round-trips fully typed without needing the entity struct on read.
- Row bodies stored as variant-tagged CBOR triples; values written untyped get re-tagged from CBOR
  shape on read.
- Secondary indexes are opt-in via `ColumnFlag::Indexed` (new variant added in `vantage-table`).
  Index tables use composite keys `(value_bytes, id)` so non-unique columns work without list
  encoding.
- Conditions: minimal `RedbCondition` enum supporting `eq` and `in_`. The id column short-circuits
  to a direct main-table lookup; conditions on non-indexed columns panic at execution time.
- `TableSource` impl with full CRUD, atomic index maintenance inside each write transaction, and
  `related_in_condition` for cross-table relationship traversal.
- Aggregations (`sum`, `min`, `max`) and `Selectable` are intentionally not implemented — redb has
  no query language.
- Drops bincode, the local error tower, and the old `RedbColumn` / `RedbExpression` / `RedbSelect`
  types.
