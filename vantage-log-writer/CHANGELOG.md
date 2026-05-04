# Changelog

## 0.4.0 — 2026-05-04

- Initial release. New [`LogWriter`](https://docs.rs/vantage-log-writer/0.4.0/vantage_log_writer/struct.LogWriter.html) data source: append-only JSONL files, one per table at `{base_dir}/{table_name}.jsonl`. Inserts are queued onto a background tokio task so callers see channel-send latency, not disk latency.
- Read methods (`list`, `get`, `count`, …) return [`ErrorKind::Unsupported`](https://docs.rs/vantage-core/0.4.1/vantage_core/enum.ErrorKind.html) — this is intentional. Pair a `LogWriter` table with a separate read-side store (Surreal, redb, …) when you need queries.
- Fields not declared as columns on the `Table` are dropped on insert. Records are projected onto the column set, and an id column is filled from the record (string or number) or generated as a [ULID](https://github.com/ulid/spec) when absent. The id column name defaults to `"id"` and can be overridden via `LogWriter::with_id_column`.
- Opt-in [`vista`](https://docs.rs/vantage-log-writer/0.4.0/vantage_log_writer/vista/index.html) feature: turn a typed `Table<LogWriter, E>` into a [`Vista`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/struct.Vista.html) via `writer.vista_factory().from_table(table)`, or build one from YAML via `from_yaml(...)`. Capabilities advertise `can_insert: true` only.
- YAML specs accept an optional `log_writer.filename` block to override the file stem when the spec's `name` differs from the on-disk filename.
