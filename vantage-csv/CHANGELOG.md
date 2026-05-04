# Changelog

## 0.4.5 — 2026-05-03

- [`CsvVistaFactory::from_yaml`](https://docs.rs/vantage-csv/0.4.5/vantage_csv/struct.CsvVistaFactory.html#method.from_yaml) now works: parse a `*.vista.yaml` describing columns, flags, and a `csv: { path }` block, get back a `Vista`. Per-column `csv: { source: <header> }` overrides the source header when it differs from the spec name.
- New `read_csv_with_variants` internal path decouples reading from a typed `Table<E>`, used by the YAML loader.
- Bumps minimum [`vantage-vista`](https://docs.rs/vantage-vista/0.4.1/) requirement to 0.4.1 (uses the new `VistaSpec` / `flags` API).

## 0.4.4 — 2026-05-03

- New opt-in [`vista`](https://docs.rs/vantage-csv/0.4.4/vantage_csv/struct.CsvVistaFactory.html) feature: turn a typed `Table<Csv, E>` into a [`Vista`](https://docs.rs/vantage-vista/0.4.0/vantage_vista/struct.Vista.html) via `csv.vista_factory().from_table(table)` and read rows as `ciborium::Value` records through the universal data handle.
- Read-only by design — `list`, `get`, `get_some`, and `count` work (with `eq` filters); writes return a clear "CSV is a read-only data source" error and `VistaCapabilities` advertise `can_count: true` only.
- Existing `TableSource` / `AnyTable` paths are unchanged; the feature is off by default.

## 0.4.3 — 2026-04-25

- `From`/`Into<ciborium::Value>` impls on `AnyCsvType` so CSV tables can be wrapped via `AnyTable::from_table`. Round-trips via `serde_json::Value` (same lossy bits as the existing JSON bridge — binary, NaN, etc.).
- Pins `vantage-table = "0.4.4"` to keep the pair in lock-step.
