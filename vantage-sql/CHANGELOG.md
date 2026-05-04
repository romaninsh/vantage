# Changelog

## 0.4.4 — 2026-05-04

- New optional `vista` feature wires SQLite, Postgres, and MySQL into [`vantage-vista`](https://docs.rs/vantage-vista). Call `db.vista_factory().from_table(table)` to expose any typed `Table<…>` as a `Vista`, or load a YAML spec via `build_from_spec` for config-driven setups.
- Each backend ships its own `*VistaSpec` / `*VistaFactory` / `*TableShell` triple under `mysql::vista`, `postgres::vista`, and `sqlite::vista`, with full read/write/count capabilities and `eq` filtering through the existing typed-column path.
- Backend-specific `sqlite:` / `postgres:` / `mysql:` blocks in the YAML spec let you override table and column names without leaving the spec.

## 0.4.3 — 2026-04-19

- SQL `is_null` / `is_not_null` operations rendered as `{} IS NULL` / `{} IS NOT NULL` for sqlite, postgres, mysql.
- Doc fixes in `docs4`.
