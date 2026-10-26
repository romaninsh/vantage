# Changelog

## 0.8.2 — 2026-10-26

- Tests of one behaviour are folded into one test each (reopening a file, insert ids, deletes,
  conditions, `in_`, indexes, conditioned reads, paging) and tests another test already covers
  are dropped: 139 tests become 120 with the same coverage. No API or behaviour change.

## 0.8.1 — 2026-10-25

- Tests whose assertions other tests already make are dropped. No API or behaviour change.

## 0.8.0 — 2026-10-18

- Requires vantage-table 3.0.0.
- **Breaking:** `RedbCondition::DeferredIn { column, deferred }`: what `related_in_condition` now
  returns, so the condition names its column. It resolves like `Deferred`, and `column()` returns
  the column. Upgrade: an exhaustive `match` on `RedbCondition` needs the new arm.
- `in_values_condition` builds `RedbCondition::In`; `describe_condition` and `condition_column`
  are implemented.

## 0.7.3 — 2026-10-14

- The in-crate unit tests move to the blackbox suite under `tests/`, tests that covered nothing
  another test didn't are dropped, and public behaviour the docs promise but nothing tested gets a
  test. No API or behaviour change.

## 0.7.2 — 2026-10-14

- Requires vantage-table 2.0.0, vantage-expressions 1.2.0 and vantage-dataset 1.1.2.

## 0.7.1 — 2026-10-07

- `Redb::execute` builds on `vantage_expressions::execute_strict`; its errors name the backend in a `backend` context key.

## 0.7.0 — 2026-10-05

- Fixed: insert of an existing id overwrote the row and left stale index entries; it now returns the
  stored row (an id outside the set is `Conflict`).
- Fixed: `insert_return_id` dropped a record-supplied id; it keeps it, else mints a UUIDv7.
- **Breaking:** by-id reads and writes honour the table's conditions; delete outside the set is `Ok`;
  patch of a missing row is `NotFound`; a condition that can't be evaluated (search) refuses the write.

## 0.6.3 — 2026-10-04

- Id column name comes from `Table::id_field_name()`.

## 0.6.2 — 2026-10-03

- Depends on `vantage-core` 1.0.0, `vantage-types` 1.0.0, `vantage-expressions` 1.0.0,
  `vantage-dataset` 1.0.0, `vantage-table` 1.0.0.

## 0.6.1 — 2026-06-25

- `AnyRedbType` declares `null_when: ciborium::Value::Null`, so it implements `InvariantValue`
  (vantage-types 0.6.2) and participates in vantage-table's set-invariant enforcement.

## 0.6.0 — 2026-06-17

- Coordinated 0.6 release; internal dependencies realigned to 0.6. No public API changes.

## 0.5.4 — 2026-06-13

- Full-table search now reports the canonical `Unsupported` error kind, matching how the other
  non-searchable backend (CSV) signals the same limitation. No behavior change beyond error
  classification.

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
