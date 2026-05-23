# Changelog

## 0.5.3 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.5.2 — 2026-05-23

- Drops the `vantage_table::any::AnyTable` re-export from `prelude` — `AnyTable` is deleted upstream
  in [vantage-table 0.5.2](https://docs.rs/vantage-table/0.5.2/vantage_table/). Use the driver's
  `vista_factory().from_table(...)` for cross-driver wrapping.

## 0.5.1 — 2026-05-23

- Tracks [vantage-dataset 0.5.0](https://docs.rs/vantage-dataset/0.5/vantage_dataset/)'s `ImTable` /
  `ImDataSource` parametrization. No public API change in this crate.

## 0.5.0 — 2026-05-23

- Bumped to the 0.5 line to track
  [vantage-table 0.5.0](https://docs.rs/vantage-table/0.5.0/vantage_table/)'s opening of the
  `AnyTable` decommission cycle. No code changes beyond the dependency pin.

## 0.4.9 — 2026-05-18

- Tracks [vantage-vista 0.4.10](https://docs.rs/vantage-vista/0.4.10/vantage_vista/)'s
  schema-on-source refactor. Each SQL `*TableShell` now owns its
  [`VistaMetadata`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/struct.VistaMetadata.html)
  and implements the new `columns` / `references` / `id_column` shell methods. Factory entry points
  (`db.vista_factory().from_table(...)` / `from_yaml(...)`) are unchanged.
- Pins `vantage-vista = "0.4.10"`.

## 0.4.8 — 2026-05-17

- All three SQL `*TableShell`s ship the full Stage 5 query surface:
  [`add_order`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_order)
  on any column (every column gets the
  [`ORDERABLE`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/flags/constant.ORDERABLE.html)
  flag at factory time),
  [`add_search`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_search)
  via the existing `search_table_condition`, and offset-style pagination
  ([`set_page_size`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.set_page_size) +
  [`fetch_page`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.fetch_page)
  /
  [`fetch_next`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.fetch_next),
  encoding the cursor as a 1-based page number).
- Capabilities updated: `can_order`, `can_search`, `can_set_page_size`, `can_fetch_page`,
  `can_fetch_next` all `true`. The retired `paginate_kind` flag is gone — drop it from any direct
  `VistaCapabilities` construction.
- Pins `vantage-vista = "0.4.9"`, `vantage-table = "0.4.12"`.

## 0.4.7 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.4.6 — 2026-05-16

- All three SQL `*TableShell`s implement
  [`TableShell::get_ref`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/trait.TableShell.html#method.get_ref)
  and `get_ref_kinds`: row-based reference traversal at the Vista layer. Each shell converts the
  CBOR parent row into the driver's `Any*Type` map, calls `Reference::resolve_from_row` on the
  wrapped typed table, and re-wraps the result via the driver's own `VistaFactory`.
- `eq_value_condition` implemented on `SqliteDB`, `PostgresDB`, `MysqlDB` via their respective
  `*Operation::eq` traits, returning the driver's native condition type.
- Integration tests in `tests/sqlite/6_vista.rs` exercise the new path end-to-end against in-memory
  SQLite: same-driver `has_many` traversal, `Vista::list_references` cardinality, and the
  `Vista::with_foreign` lazy-closure invariant.
- Pins `vantage-vista = "0.4.7"`, `vantage-table = "0.4.10"`.

## 0.4.5 — 2026-05-09

- Pins `vantage-types` to `>= 0.4.2`. The `RichText`-returning `TerminalRender` impls landed in
  0.4.4 alongside `vantage-types 0.4.2`; without an explicit floor, cargo could resolve
  `vantage-types` to 0.4.0/0.4.1 and fail to compile against the old trait shape.

## 0.4.4 — 2026-05-04

- New optional `vista` feature wires SQLite, Postgres, and MySQL into
  [`vantage-vista`](https://docs.rs/vantage-vista). Call `db.vista_factory().from_table(table)` to
  expose any typed `Table<…>` as a `Vista`, or load a YAML spec via `build_from_spec` for
  config-driven setups.
- Each backend ships its own `*VistaSpec` / `*VistaFactory` / `*TableShell` triple under
  `mysql::vista`, `postgres::vista`, and `sqlite::vista`, with full read/write/count capabilities
  and `eq` filtering through the existing typed-column path.
- Backend-specific `sqlite:` / `postgres:` / `mysql:` blocks in the YAML spec let you override table
  and column names without leaving the spec.
- `from_table` now preserves the original entity type instead of erasing to `EmptyEntity` —
  `Table<Db, E>` survives the wrap so user-defined `with_expression` closures parameterised over `E`
  still typecheck. The boxed `TableShell` in `Vista` keeps the dyn-erasure boundary at one place.
- `*TableShell` implements
  [`driver_name`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/trait.TableShell.html#method.driver_name)
  so
  [`Vista::driver()`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/struct.Vista.html#method.driver)
  reports `"sqlite"` / `"postgres"` / `"mysql"` for diagnostics.
- Bumps minimum [`vantage-vista`](https://docs.rs/vantage-vista/0.4.4/) requirement to 0.4.4.

## 0.4.3 — 2026-04-19

- SQL `is_null` / `is_not_null` operations rendered as `{} IS NULL` / `{} IS NOT NULL` for sqlite,
  postgres, mysql.
- Doc fixes in `docs4`.
