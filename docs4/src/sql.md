# SQL: PostgreSQL, MySQL & SQLite

The `vantage-sql` crate provides persistence implementations for three SQL databases via
[sqlx](https://github.com/launchbadge/sqlx). All three share the same architecture — CBOR-based type
systems, expression engine, query builder, and full `TableSource` implementation — but each has
vendor-specific behaviour around type affinity, quoting, and parameter binding.

## Backends

| Backend    | Struct       | Type System       | Param style | ID quoting       |
| ---------- | ------------ | ----------------- | ----------- | ---------------- |
| PostgreSQL | `PostgresDB` | `AnyPostgresType` | `$1, $2`    | `"double_quote"` |
| MySQL      | `MysqlDB`    | `AnyMysqlType`    | `?, ?`      | `` `backtick` `` |
| SQLite     | `SqliteDB`   | `AnySqliteType`   | `?1, ?2`    | `"double_quote"` |

`MysqlDB` supports MySQL 8 and MariaDB 11; both run the same test suite in CI.
MariaDB differences: `JSON_EXTRACT` / `.as_json()` returns quoted JSON text; there is no `GROUPING()`, no
`ORDER BY` with `WITH ROLLUP`, and no `REGEXP_LIKE` (use `REGEXP`); `DateTime<FixedOffset>` binds are rejected.

## What they implement

All three implement the full trait stack:

- `DataSource` — marker
- `ExprDataSource` — parametric SQL execution with CBOR values
- `SelectableDataSource` — query builder with JOINs, CTEs, window functions
- `TableSource` — full CRUD, columns, conditions, aggregates
- `TableQuerySource` — table definition → full query

## The Vista

With the `vista` feature, each backend's factory builds a [Vista](./intro/step4-vista.md) from a
YAML spec or a typed table. All three advertise the same reads, because SQL can do each of them in
the query: `Order`, `Search`, `FilterOps`, `Window` (a `LIMIT`/`OFFSET` window, served by
`fetch_window`) and `Traverse`. A table-sourced Vista adds the writes, `Insert`, `Update`, `Delete`
and `ConfineWrites`. A query-sourced one (`rhai:` or `base:`) takes no writes, because no single
table could receive them. Per backend:

- **PostgreSQL** adds `Subscribe` when the factory is built `with_notify(true)`. Postgres can't
  tell whether a `{table}_changed` trigger exists, so the application has to say so.
- **MySQL** adds `NativeConditions` (the `with_condition` verb in `modify:` scripts) and, on a
  table-sourced Vista, `Import`.

A quicksearch is a `LIKE` on each searchable column, joined with `OR`. The searchable columns are
the ones flagged `searchable`, or every column when none is flagged. Every driver reads that rule
from `Table::search_columns` (`VistaMetadata::search_columns` states it over a Vista's columns), so
no backend picks its own. Imported reference columns are
left out, because they exist only as projection aliases, and a table with nothing to search matches
no rows. PostgreSQL uses `ILIKE` on the column cast to text. SQLite's `LIKE` already ignores case
for ASCII text, and MySQL's follows the column's collation.

## CBOR value representation

All SQL backends use **CBOR** (`ciborium::Value`) as their internal value type — not JSON. This
preserves type fidelity that JSON loses:

- **Integer vs Float** — JSON's `42` is ambiguous; CBOR distinguishes `Integer(42)` from
  `Float(42.0)`
- **Binary data** — CBOR has native byte arrays; JSON would need base64 encoding
- **Precise decimals** — stored as tagged CBOR values, not lossy floats

Values are converted to CBOR on write (via the type system's `to_cbor()`) and read back as untyped
CBOR (via `from_cbor()`). The type markers from `vantage_type_system!` ensure correct binding —
integers bind as `i64`, text as `&str`, booleans as `bool`.

## Type conversion reference

Each database handles Rust types differently depending on the column type. See the
[Type Conversions](./sql/type-conversions.md) reference for detailed round-trip tables covering:

- **Chrono types** — `NaiveDate`, `NaiveTime`, `NaiveDateTime`, `DateTime<Utc>`,
  `DateTime<FixedOffset>`
- **Numeric types** — `Decimal`, `i64`, `f64`
- Exact vs lossy vs truncated behaviour per column type
- Cross-type coercion rules and error cases
