# Type Conversions

How Rust native types round-trip through different SQL column types.
Tested via entity insert + read-back (Table API).

Legend:
- **exact** = lossless round-trip, value identical after insert + read
- **lossy** = value stored but precision reduced (e.g. f64 rounding)
- **truncated** = value stored but digits beyond column precision are cut
- **err** = database rejects the insert or entity conversion fails on read

## Chrono Types

### MySQL

[tests/mysql/1_chrono.rs](tests/mysql/1_chrono.rs)

| Column \ Rust type | NaiveDate | NaiveTime | NaiveDateTime | DateTime\<Utc\> | DateTime\<FixedOffset\> |
|---|---|---|---|---|---|
| VARCHAR | exact | exact | exact | exact | exact |
| DATE | exact | err | err | err | — |
| TIME | err | truncating µs | err | err | — |
| TIME(6) | err | exact | err | err | — |
| DATETIME | err | err | truncating µs | truncating µs | offset → +00:00 |
| DATETIME(6) | err | err | exact | exact | — |
| TIMESTAMP | err | err | truncating µs | truncating µs | offset → +00:00 |
| TIMESTAMP(6) | err | err | exact | exact | — |

- Format: `"2025-01-10 12:00:00"` (space separator, no T); `FixedOffset` appends `+05:30`
- TIME/DATETIME/TIMESTAMP default to 0 fractional digits — use `(6)` for microseconds
- `DateTime<FixedOffset>`: VARCHAR preserves offset; DATETIME/TIMESTAMP normalize to UTC
- Cross-type coercions (e.g. NaiveTime → DATE) fail with variant mismatch

### PostgreSQL

[tests/postgres/1_chrono.rs](tests/postgres/1_chrono.rs)

| Column \ Rust type | NaiveDate | NaiveTime | NaiveDateTime | DateTime\<Utc\> | DateTime\<FixedOffset\> |
|---|---|---|---|---|---|
| VARCHAR | exact | exact | exact | exact | offset → +00:00 |
| DATE | exact | err | err | err | — |
| TIME | err | exact | err | err | — |
| TIMESTAMP | err | err | exact | exact | offset → +00:00 |
| TIMESTAMPTZ | err | err | exact | exact | offset → +00:00 |

- Format: `"2025-01-10 12:00:00+00"` (space separator, abbreviated tz offset)
- Microsecond precision by default — no `(6)` suffix needed
- Typed binds (chrono types, not text) required for DATE/TIME/TIMESTAMP/TIMESTAMPTZ
- `DateTime<FixedOffset>`: offset always normalized to UTC (typed binds), no column preserves it
- Cross-type coercions fail with variant mismatch

### SQLite

[tests/sqlite/1_chrono.rs](tests/sqlite/1_chrono.rs)

| Column \ Rust type | NaiveDate | NaiveTime | NaiveDateTime | DateTime\<Utc\> | DateTime\<FixedOffset\> |
|---|---|---|---|---|---|
| TEXT | exact | exact | exact | exact | exact |

- All dates stored as TEXT — format: ISO 8601 with T separator (`"2025-01-10T12:00:00Z"`)
- Subsecond precision and timezone offsets preserved as-is

## Numeric Types

### MySQL

[tests/mysql/1_decimal.rs](tests/mysql/1_decimal.rs)

| Column \ Rust type | Decimal | i64 | f64 |
|---|---|---|---|
| VARCHAR | exact | exact | exact |
| DECIMAL(20,6) | truncated to 6 places | err | err |
| DECIMAL(38,15) | exact | err | err |
| DOUBLE | lossy (~15 digits) | err | exact |
| FLOAT | lossy (~7 digits) | err | lossy (~7 digits) |
| BIGINT | fractional part lost | exact | err |

- No cross-conversion between Integer, Float, and Decimal CBOR types
- VARCHAR works for all types — `from_cbor` parses text as fallback

### PostgreSQL

[tests/postgres/1_decimal.rs](tests/postgres/1_decimal.rs)

| Column \ Rust type | Decimal | i64 | f64 |
|---|---|---|---|
| VARCHAR | exact | exact | exact |
| NUMERIC(20,6) | truncated to 6 places | err | err |
| NUMERIC(38,15) | exact | err | err |
| DOUBLE PRECISION | lossy (~15 digits) | err | exact |
| REAL | lossy (~7 digits) | err | lossy (~7 digits) |
| BIGINT | fractional part lost | exact | err |

- Typed binds (`rust_decimal::Decimal`) used for NUMERIC columns

### SQLite

[tests/sqlite/1_decimal.rs](tests/sqlite/1_decimal.rs)

| Column \ Rust type | Decimal | i64 | f64 |
|---|---|---|---|
| TEXT | exact | exact | exact |
| NUMERIC | lossy (~15 digits, stored as REAL) | exact | exact |
| REAL | lossy (~15 digits) | err | exact |
| INTEGER | fractional part lost | exact | err |

- SQLite infers CBOR type from stored value, not declared column type
- NUMERIC/REAL affinities coerce to float — `i64` in REAL comes back as Float and fails
- Store Decimal in TEXT for lossless precision
