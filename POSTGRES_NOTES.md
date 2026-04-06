# PostgreSQL Backend Implementation Notes

## Overview

PostgreSQL support is implemented as a feature-gated module (`postgres`) in the `vantage-sql` crate,
following the same architecture as the existing SQLite backend. Enable with `--features postgres`.

## Key Differences from SQLite

### Type System

PostgreSQL has a richer type system than SQLite. The variants defined are:

| Variant | Rust types | PostgreSQL types |
|---------|-----------|-----------------|
| Null | - | NULL |
| Bool | bool | BOOLEAN |
| Int2 | i8, i16, u8 | SMALLINT |
| Int4 | i32, u16 | INTEGER |
| Int8 | i64, u32 | BIGINT |
| Float4 | f32 | REAL |
| Float8 | f64 | DOUBLE PRECISION |
| Text | String, &str | TEXT, VARCHAR |
| Bytea | Vec<u8> | BYTEA |

Unlike SQLite where bool maps to Integer (0/1), PostgreSQL has native BOOLEAN. This means
`AnyPostgresType::new(true)` stores `Value::Bool(true)` (not `Value::Number(1)`).

Integer widths (Int2/Int4/Int8) are distinct variants — `AnyPostgresType::new(42i32)` gets
variant `Int4`, which blocks `try_get::<i64>()` (Int4 ≠ Int8). Untyped values (from DB reads)
bypass this check as expected.

### Parameter Binding

PostgreSQL uses `$1`, `$2`, ... for positional parameters (not `?1`, `?2` like SQLite).
The `prepare_typed_query` function in `expr_data_source.rs` handles this conversion.

### NUMERIC Type Handling

PostgreSQL's `SUM()`, `AVG()` on integer columns return `NUMERIC`, which sqlx cannot decode
without the `bigdecimal` or `rust_decimal` feature. The `as_aggregate` method wraps results
with `CAST(... AS BIGINT)` to ensure decodability.

The `pg_column_to_json` row reader also handles `NUMERIC` by trying i64/i32/f64 decoders
in sequence.

### UPSERT (replace)

SQLite uses `INSERT OR REPLACE INTO`. PostgreSQL uses
`INSERT ... ON CONFLICT (id) DO UPDATE SET col = EXCLUDED.col`.
The `replace_table_value` method builds this automatically.

### ID Type Handling

When the table uses `SERIAL` (integer) IDs, the id string needs to be bound as an integer,
not text. The `id_value()` helper parses the string id and creates the appropriate typed
`AnyPostgresType` (Int8 for numeric ids, Text otherwise).

### Search Expression

`search_table_expr` uses `"col"::text LIKE $1` instead of just `"col" LIKE $1` to handle
non-text columns (PostgreSQL doesn't implicitly cast to text for LIKE).

## Docker Setup

Tests require a running PostgreSQL instance. Scripts are in `scripts/postgres/`:

```bash
# Start PostgreSQL on port 5433 (avoids conflict with local postgres on 5432)
./scripts/postgres/start.sh

# Load bakery test data
./scripts/postgres/ingress.sh

# Stop
./scripts/postgres/stop.sh
```

Port 5433 is used because many dev machines have a local PostgreSQL on 5432.

## Test Structure

All 84 tests follow the same pattern as SQLite:

- `1_types_round_trip.rs` — 18 tests: AnyPostgresType in-memory round-trips
- `1_types_record.rs` — 9 tests: Record<AnyPostgresType> typed/untyped
- `2_expressions.rs` — 7 tests: ExprDataSource execute with parameters
- `2_insert.rs` — 4 tests: INSERT with typed parameters
- `2_records.rs` — 5 tests: SELECT into Record, entity deserialization
- `2_defer.rs` — 3 tests: cross-database deferred expressions
- `2_associated.rs` — 4 tests: AssociatedExpression with typed results
- `3_select.rs` — 14 tests: PostgresSelect builder rendering + live execution
- `4_table_def.rs` — 1 test: Table query generation
- `4_readable_data_set.rs` — 3 tests: list, get, get_some
- `4_conditions.rs` — 3 tests: custom conditions, field comparison, Operation::eq
- `4_aggregates.rs` — 4 tests: COUNT, SUM, MAX, MIN
- `4_editable_data_set.rs` — 6 tests: insert, replace, patch, delete, delete_all, insert_return_id
- `5_references.rs` — 3 tests: has_many, has_one relationship traversal

Each test that touches the database uses a unique table name to avoid race conditions
from parallel test execution.

## File Structure

```
vantage-sql/src/postgres/
├── mod.rs          — PostgresDB struct with pool + aggregate method
├── macros.rs       — postgres_expr! macro
├── operation.rs    — blanket impl note
├── row.rs          — bind_postgres_value + row_to_record
├── types/
│   ├── mod.rs      — vantage_type_system! macro + variant detection
│   ├── bool.rs     — bool -> Bool variant
│   ├── numbers.rs  — integer/float types
│   ├── string.rs   — String/&str -> Text
│   └── value.rs    — From, Display, Expressive, TryFrom impls
├── impls/
│   ├── mod.rs      — DataSource marker
│   ├── expr_data_source.rs  — execute + defer + $N placeholder conversion
│   ├── selectable_data_source.rs
│   └── table_source.rs      — full TableSource implementation
└── statements/
    ├── mod.rs
    ├── select/
    │   ├── mod.rs       — PostgresSelect struct
    │   ├── render.rs    — SQL rendering
    │   ├── join.rs      — JOIN clause
    │   └── impls/
    │       ├── mod.rs
    │       └── selectable.rs — Selectable trait impl + CAST aggregate
    ├── insert/
    │   ├── mod.rs, builder.rs, render.rs
    ├── update/
    │   ├── mod.rs, builder.rs, render.rs
    └── delete/
        ├── mod.rs, builder.rs, render.rs
```
