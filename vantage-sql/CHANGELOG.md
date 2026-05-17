# Changelog

## 0.4.7 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.4.6 — 2026-05-16

- All three SQL `*TableShell`s implement [`TableShell::get_ref`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/trait.TableShell.html#method.get_ref) and `get_ref_kinds`: row-based reference traversal at the Vista layer. Each shell converts the CBOR parent row into the driver's `Any*Type` map, calls `Reference::resolve_from_row` on the wrapped typed table, and re-wraps the result via the driver's own `VistaFactory`.
- `eq_value_condition` implemented on `SqliteDB`, `PostgresDB`, `MysqlDB` via their respective `*Operation::eq` traits, returning the driver's native condition type.
- Integration tests in `tests/sqlite/6_vista.rs` exercise the new path end-to-end against in-memory SQLite: same-driver `has_many` traversal, `Vista::list_references` cardinality, and the `Vista::with_foreign` lazy-closure invariant.
- Pins `vantage-vista = "0.4.7"`, `vantage-table = "0.4.10"`.

## 0.4.5 — 2026-05-09

- Pins `vantage-types` to `>= 0.4.2`. The `RichText`-returning `TerminalRender` impls landed in 0.4.4 alongside `vantage-types 0.4.2`; without an explicit floor, cargo could resolve `vantage-types` to 0.4.0/0.4.1 and fail to compile against the old trait shape.

## 0.4.4 — 2026-05-04

- New optional `vista` feature wires SQLite, Postgres, and MySQL into [`vantage-vista`](https://docs.rs/vantage-vista). Call `db.vista_factory().from_table(table)` to expose any typed `Table<…>` as a `Vista`, or load a YAML spec via `build_from_spec` for config-driven setups.
- Each backend ships its own `*VistaSpec` / `*VistaFactory` / `*TableShell` triple under `mysql::vista`, `postgres::vista`, and `sqlite::vista`, with full read/write/count capabilities and `eq` filtering through the existing typed-column path.
- Backend-specific `sqlite:` / `postgres:` / `mysql:` blocks in the YAML spec let you override table and column names without leaving the spec.
- `from_table` now preserves the original entity type instead of erasing to `EmptyEntity` — `Table<Db, E>` survives the wrap so user-defined `with_expression` closures parameterised over `E` still typecheck. The boxed `TableShell` in `Vista` keeps the dyn-erasure boundary at one place.
- `*TableShell` implements [`driver_name`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/trait.TableShell.html#method.driver_name) so [`Vista::driver()`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/struct.Vista.html#method.driver) reports `"sqlite"` / `"postgres"` / `"mysql"` for diagnostics.
- Bumps minimum [`vantage-vista`](https://docs.rs/vantage-vista/0.4.4/) requirement to 0.4.4.

## 0.4.3 — 2026-04-19

- SQL `is_null` / `is_not_null` operations rendered as `{} IS NULL` / `{} IS NOT NULL` for sqlite, postgres, mysql.
- Doc fixes in `docs4`.
