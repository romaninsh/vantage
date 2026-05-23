# Changelog

## 0.5.2 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.5.0 — 2026-05-23

- Bumped to the 0.5 line to track
  [vantage-table 0.5.0](https://docs.rs/vantage-table/0.5.0/vantage_table/)'s opening of the
  `AnyTable` decommission cycle. No code changes beyond the dependency pin.

## 0.4.2 — 2026-05-18

- Tracks [vantage-vista 0.4.10](https://docs.rs/vantage-vista/0.4.10/vantage_vista/)'s
  schema-on-source refactor. `LogWriterTableShell` now owns its
  [`VistaMetadata`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/struct.VistaMetadata.html)
  and implements the new `columns` / `references` / `id_column` shell methods.
  `writer.vista_factory().from_table(...)` / `from_yaml(...)` surface unchanged.
- Pins `vantage-vista = "0.4.10"`.

## 0.4.1 — 2026-05-15

Compatibility with change in vantage-vista

## 0.4.0 — 2026-05-04

- Initial release. New
  [`LogWriter`](https://docs.rs/vantage-log-writer/0.4.0/vantage_log_writer/struct.LogWriter.html)
  data source: append-only JSONL files, one per table at `{base_dir}/{table_name}.jsonl`. Inserts
  are queued onto a background tokio task so callers see channel-send latency, not disk latency.
- Read methods (`list`, `get`, `count`, …) return
  [`ErrorKind::Unsupported`](https://docs.rs/vantage-core/0.4.1/vantage_core/enum.ErrorKind.html) —
  this is intentional. Pair a `LogWriter` table with a separate read-side store (Surreal, redb, …)
  when you need queries.
- Fields not declared as columns on the `Table` are dropped on insert. Records are projected onto
  the column set, and an id column is filled from the record (string or number) or generated as a
  [ULID](https://github.com/ulid/spec) when absent. The id column name defaults to `"id"` and can be
  overridden via `LogWriter::with_id_column`.
- Opt-in [`vista`](https://docs.rs/vantage-log-writer/0.4.0/vantage_log_writer/vista/index.html)
  feature: turn a typed `Table<LogWriter, E>` into a
  [`Vista`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/struct.Vista.html) via
  `writer.vista_factory().from_table(table)`, or build one from YAML via `from_yaml(...)`.
  Capabilities advertise `can_insert: true` only.
- YAML specs accept an optional `log_writer.filename` block to override the file stem when the
  spec's `name` differs from the on-disk filename.
