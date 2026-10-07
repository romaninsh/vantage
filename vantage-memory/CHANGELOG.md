# Changelog

## 1.2.0 — 2026-10-05

- **Breaking:** typed and Vista writes follow the write contract: insert of an existing id returns
  the stored row (an id outside the set is `Conflict`); delete of a missing row or one outside the
  set is `Ok`; replace of a missing row creates it; patch of a missing row is `NotFound`.
- Writes through a narrowed table or Vista are confined to its conditions and fill its equality
  conditions; has-many traversal fills the foreign key.
- **Breaking:** native import (`import_vista_values`) never overwrites: an existing id counts zero,
  and a row outside the set fails the import before anything is written.
- Depends on `vantage-table` 1.2.0, `vantage-dataset` 1.1.0.
- Vista shells advertise `can_confine_writes`.

## 1.1.1 — 2026-10-04

- The vista factory uses the shared spec helpers.
- Depends on `vantage-table` 1.1.1, `vantage-vista` 1.1.1.

## 1.1.0 — 2026-10-03

- Spec `lazy:` columns, computed by the Vista (`rhai` feature).
- Depends on `vantage-table` 1.1.0, `vantage-vista` 1.1.0.

## 1.0.0 — 2026-10-03

Initial release.

- In-memory datasource: sync store with per-table locks, `Arc`-shared rows and hash indexes.
- Typed `TableSource` (`MemoryDB`) with every `FilterOp`, search, ordering, paging, count/sum/min/max and references.
- Vista `MemoryTableShell` + `MemoryVistaFactory` (`memory: { indexed, seed }`), with native `watch_vista` and `upsert_vista_value`.
- Seed load/dump from JSON or YAML.
- `set_quiet` takes the table's write lock, so a write never loses its event or strands a `Reset` while quiet mode flips.
- A missing row is `ErrorKind::NotFound` on replace, patch and delete.
- Depends on `vantage-core` 1.0.0, `vantage-types` 1.0.0, `vantage-expressions` 1.0.0, `vantage-dataset` 1.0.0, `vantage-table` 1.0.0, `vantage-vista` 1.0.0.
