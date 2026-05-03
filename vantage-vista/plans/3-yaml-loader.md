# Stage 3 — Universal YAML loader

Status: **Done**

Add YAML → Vista construction. Universal vocabulary (table name,
columns, flags, references) is parsed by `vantage-vista`; driver-specific
extras are carried through three generic parameters on `VistaSpec` and
deserialised by the driver itself.

## Discussion phase

Confirmed with the user:

- [x] `VistaSpec` shape — universal fields only (`name`, `datasource`,
      `id_column`, `columns`, `references`). `title_columns` dropped:
      title membership lives only as a column flag.
- [x] Driver-specific YAML lives under top-level keys named by the driver,
      both at the table level (e.g. `csv:`) and at the column level
      (matches vantage-ui's existing pattern). Each driver supplies its
      own typed block via `VistaSpec`'s three generic parameters.
- [x] Flag vocabulary is open `Vec<String>`. Constants in
      `vantage_vista::flags` (`ID`, `TITLE`, `SEARCHABLE`, `MANDATORY`,
      `HIDDEN`) name the values vista's own accessors understand;
      drivers and consumers may add their own.
- [x] Reference kinds: `has_one`, `has_many`, `has_foreign`. Sugar form
      `references: products` parses as a `has_one`-style hint.
- [x] Errors are wrapped via `serde_yaml_ng::Error` → `vantage_core::Error`.
      Driver-specific blocks set `deny_unknown_fields` to surface typos.

## Scope

In:

- `VistaSpec<T, C, R>` generic over driver-specific table/column/reference
  blocks; `NoExtras` placeholder for drivers with none.
- `VistaFactory::Extras` associated types, `build_from_spec`, and a
  default `from_yaml` that parses with `serde_yaml_ng` then dispatches.
- CSV driver: `CsvTableExtras` (`csv: { path }`), `CsvColumnExtras`
  (`csv: { source }`), `NoExtras` for references.

Out:

- Hooks block (stage 6)
- Conditions block (stage 5)
- Coop integration (stage 7)
- Cache key derivation (deferred — comes with stage 7)

## Plan

- [x] Discuss with user: VistaSpec shape, extras placement
- [x] Define `VistaSpec<T, C, R>` struct + serde derive (with bound
      attributes for the three generics)
- [x] Define `flags` constants module (`ID`, `TITLE`, `HIDDEN`, etc.) —
      flags themselves stay open `Vec<String>`
- [x] Define `ColumnSpec<C>` (type, flags, references sugar, driver block)
- [x] Define `ReferenceSpec<R>` (table, kind, foreign_key, driver block)
      and `ReferenceSugar` (untagged sugar/full enum)
- [x] Add `VistaFactory::Extras` associated types + default `from_yaml`
      that parses then calls `build_from_spec`
- [x] CSV driver implements the spec types + `build_from_spec`. CSV's
      `read_csv_with_variants` decouples reading from typed `Column<E>`.
- [x] Test: YAML round-trip with CSV — load YAML, list rows, eq filter,
      reject unknown driver-block fields
- [x] Convert `Vista::Column` to carry `flags: Vec<String>` (was
      `hidden: bool`); derive `get_title_columns()` from flags

Deferred:

- [ ] Document the YAML schema in a `SCHEMA.md` (will land alongside
      stage 4's driver rollout, when more than one driver demonstrates
      the cross-driver vocabulary)

## References

- Subsumes:
  - `../../PLAN_0_5.md` §1 "Column visibility + column defaults" —
    flag vocabulary lands here, defaults later
  - `../../PLAN_0_5.md` §2 "Column (de)serialisation mapping" — surfaces
    in YAML type vocabulary
  - vantage-ui's per-driver `XColumnExtras` blocks — replaced by this
    extras pattern
