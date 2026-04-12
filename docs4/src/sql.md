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

## What they implement

All three implement the full trait stack:

- `DataSource` — marker
- `ExprDataSource` — parametric SQL execution with CBOR values
- `SelectableDataSource` — query builder with JOINs, CTEs, window functions
- `TableSource` — full CRUD, columns, conditions, aggregates
- `TableQuerySource` — table definition → full query

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
