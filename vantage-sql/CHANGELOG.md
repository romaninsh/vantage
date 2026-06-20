# Changelog

## 0.6.1 — unreleased

- SQLite Vista now implements `fetch_window` (advertised via `can_fetch_window`), serving an
  arbitrary `[offset, offset + limit)` row window through `Pagination::window`. Previously only
  page-indexed `fetch_page` was available, so random-access window fetches were refused.

## 0.6.0 — unreleased

- Coordinated 0.6 release; internal dependencies realigned to 0.6. No public API changes.

## 0.5.9 — 2026-06-07

### Changed

- `register_engine!` is split so its registrations live in a reusable
  `__register_engine_onto(&mut Engine)`; the macro and `__create_engine` call it. Prepares the
  SQL backend for the conventional Rhai-scripted reference traversal added in
  `vantage-vista` 0.5.4. No behavior change for existing engine call sites.

## 0.5.8 — 2026-06-06

### Changed

- Tracks the `vantage-vista` thin refactor (0.5.3): dropped the obsolete `with_foreign`
  vista integration test now that cross-persistence traversal lives in
  `vantage-vista-factory`. No functional change to the SQL backends.

## 0.5.7 — 2026-06-02

### Added

- Tables can be sourced from a sub-`SELECT` via `vantage-table`'s new `SelectSource`
  (`type Source = SelectSource<SqliteSelect>`), rendering `FROM (<select>) AS <alias>`.
- SQLite vista specs accept a `sqlite.rhai:` block: a Rhai script that builds the vista's
  source `SELECT` instead of pointing at a physical table. The resulting vista is read-only
  (insert/update/delete capabilities are cleared). Requires the `rhai` feature.

## 0.5.6 — 2026-06-01

### New Features

- **Rhai DSL Engine**: Write SQL queries in Rhai scripting language with full cross-database
  support. The new `rhai` feature flag enables a high-level DSL that compiles to vendor-specific SQL
  for SQLite, PostgreSQL, and MySQL. Example:

  ```rust
  let users = table("users").alias("u");
  select()
      .from(users)
      .expression(users["name"])
      .where(users["age"] >= 18)
      .order_by(users["name"], "asc")
  ```

  - Automatic identifier quoting (backticks for MySQL, double quotes for PostgreSQL/SQLite)
  - Dialect-aware primitives: `date_format()` translates to `strftime()`/`TO_CHAR()`/`DATE_FORMAT()`
  - New `group_concat()` primitive with DISTINCT support (maps to `GROUP_CONCAT`/`STRING_AGG`)
  - Comparison operators (`==`, `!=`, `<`, `>`, `<=`, `>=`) work across all backends
  - Test runner with `--fix` mode for generating SQL snapshots

- **GroupConcat Primitive**: Cross-database string aggregation with optional DISTINCT. Renders as:
  - SQLite/MySQL: `GROUP_CONCAT(DISTINCT expr, ',')`
  - PostgreSQL: `STRING_AGG(DISTINCT expr, ',')`

### Internal Changes

- Added `SelectBuilder` and `JoinBuilder` traits for database-specific select/join operations
- Refactored select builder methods into dedicated module (`src/rhai_engine/select_methods.rs`)
- Implemented comparison operators module (`src/rhai_engine/operators.rs`)
- New test infrastructure: `examples/rhai_test.rs` runner with snapshot testing support
- Added `tests/rhai-tests/` directory with `.rhai` query files and `.sql`/`.err` snapshots for all
  three backends

## 0.5.5 — 2026-05-31

- Contained relations on SQLite, PostgreSQL, and MySQL: embedded collections stored as JSON columns
  surface as editable sub-Vistas, with eager writeback patching the host column. Postgres and MySQL
  share the SQLite path verbatim. Also lowers a YAML `contained:` section in `table_from_spec`. See
  the
  [contained relations guide](https://romaninsh.github.io/vantage/new-persistence/step9-contained-relations.html).

## 0.5.4 — 2026-05-30

- The SQLite, PostgreSQL, and MySQL shells implement
  [`TableShell::get_ref_target`](https://docs.rs/vantage-vista/0.5.1/vantage_vista/trait.TableShell.html),
  and their factories populate `VistaMetadata::references` — enabling
  [vantage-vista 0.5.1](https://docs.rs/vantage-vista/0.5.1/vantage_vista/)'s nested insert through
  relations. Tracks [vantage-table 0.5.4](https://docs.rs/vantage-table/0.5.4/vantage_table/).

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
