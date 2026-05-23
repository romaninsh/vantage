# Changelog

## 0.5.2 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.5.0 — 2026-05-23

- Bumped to the 0.5 line to track
  [vantage-table 0.5.0](https://docs.rs/vantage-table/0.5.0/vantage_table/)'s opening of the
  `AnyTable` decommission cycle. No code changes beyond the dependency pin.

## 0.4.13 — 2026-05-18

- Tracks [vantage-vista 0.4.10](https://docs.rs/vantage-vista/0.4.10/vantage_vista/)'s
  schema-on-source refactor. `CsvTableShell` now owns its
  [`VistaMetadata`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/struct.VistaMetadata.html)
  and implements the new
  [`columns`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/trait.TableShell.html#tymethod.columns)
  /
  [`references`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/trait.TableShell.html#tymethod.references)
  /
  [`id_column`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/trait.TableShell.html#tymethod.id_column)
  shell methods. `csv.vista_factory().from_table(...)` / `from_yaml(...)` surface unchanged.
- Pins `vantage-vista = "0.4.10"`.

## 0.4.12 — 2026-05-17

Compatibility with change in vantage-vista

## 0.4.11 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.4.10 — 2026-05-16

- `CsvTableShell` implements
  [`TableShell::get_ref`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/trait.TableShell.html#method.get_ref)
  and `get_ref_kinds`: row-based reference traversal at the Vista layer. Builds the target table
  from `Reference::resolve_from_row`, re-wraps via `CsvVistaFactory::from_table`.
- `Csv::eq_value_condition` implemented via `CsvOperation::eq`.
- Pins `vantage-vista = "0.4.7"`, `vantage-table = "0.4.10"`.

## 0.4.9 — 2026-05-09

- Pins `vantage-types` to `>= 0.4.2`. The `AnyCsvType` `TerminalRender` impl returns `RichText` and
  needs the trait shape from `vantage-types 0.4.2`.

## 0.4.8 — 2026-05-04

- Implements
  [`TableShell::driver_name`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/trait.TableShell.html#method.driver_name)
  — `Vista::driver()` reports `"csv"` for tables wrapped through `csv.vista_factory()`.
- Bumps minimum [`vantage-vista`](https://docs.rs/vantage-vista/0.4.4/) requirement to 0.4.4.

## 0.4.7 — 2026-05-04

- `CsvVistaSource` is now
  [`CsvTableShell`](https://docs.rs/vantage-csv/0.4.7/vantage_csv/struct.CsvTableShell.html),
  tracking the
  [`vantage-vista 0.4.3`](https://docs.rs/vantage-vista/0.4.3/vantage_vista/trait.TableShell.html)
  trait rename. The factory's surface is unchanged — most users go through
  `csv.vista_factory().from_table(...)` or `from_yaml(...)` and never name the source struct.

## 0.4.6 — 2026-05-04

- `add_condition_eq` filtering is now translated into an `Expression<AnyCsvType>` and pushed onto
  the wrapped `Table` instead of evaluated on Vista's side. Same surface, but consistent with the
  new condition-delegation contract.
- Internal: the `vista` module is now a directory (`vista/{mod,factory,source,spec}.rs`) for parity
  with the other drivers — `pub use` surface unchanged.
- Bumps minimum [`vantage-vista`](https://docs.rs/vantage-vista/0.4.2/) requirement to 0.4.2
  (`add_eq_condition` trait method).

## 0.4.5 — 2026-05-03

- [`CsvVistaFactory::from_yaml`](https://docs.rs/vantage-csv/0.4.5/vantage_csv/struct.CsvVistaFactory.html#method.from_yaml)
  now works: parse a `*.vista.yaml` describing columns, flags, and a `csv: { path }` block, get back
  a `Vista`. Per-column `csv: { source: <header> }` overrides the source header when it differs from
  the spec name.
- New `read_csv_with_variants` internal path decouples reading from a typed `Table<E>`, used by the
  YAML loader.
- Bumps minimum [`vantage-vista`](https://docs.rs/vantage-vista/0.4.1/) requirement to 0.4.1 (uses
  the new `VistaSpec` / `flags` API).

## 0.4.4 — 2026-05-03

- New opt-in [`vista`](https://docs.rs/vantage-csv/0.4.4/vantage_csv/struct.CsvVistaFactory.html)
  feature: turn a typed `Table<Csv, E>` into a
  [`Vista`](https://docs.rs/vantage-vista/0.4.0/vantage_vista/struct.Vista.html) via
  `csv.vista_factory().from_table(table)` and read rows as `ciborium::Value` records through the
  universal data handle.
- Read-only by design — `list`, `get`, `get_some`, and `count` work (with `eq` filters); writes
  return a clear "CSV is a read-only data source" error and `VistaCapabilities` advertise
  `can_count: true` only.
- Existing `TableSource` / `AnyTable` paths are unchanged; the feature is off by default.

## 0.4.3 — 2026-04-25

- `From`/`Into<ciborium::Value>` impls on `AnyCsvType` so CSV tables can be wrapped via
  `AnyTable::from_table`. Round-trips via `serde_json::Value` (same lossy bits as the existing JSON
  bridge — binary, NaN, etc.).
- Pins `vantage-table = "0.4.4"` to keep the pair in lock-step.
