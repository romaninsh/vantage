# Changelog

## 0.6.1 — 2026-09-29

- `set_quiet` takes the table's write lock, so a write never loses its event or strands a `Reset` while quiet mode flips.

## 0.6.0 — 2026-09-29

- In-memory datasource: sync store with per-table locks, `Arc`-shared rows and hash indexes.
- Typed `TableSource` (`MemoryDB`) with every `FilterOp`, search, ordering, paging, count/sum/min/max and references.
- Vista `MemoryTableShell` + `MemoryVistaFactory` (`memory: { indexed, seed }`), with native `watch_vista`.
- Seed load/dump from JSON or YAML.
