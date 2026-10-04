# Changelog

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
