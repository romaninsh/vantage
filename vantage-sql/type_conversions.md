# Type Conversions

How Rust native types round-trip through different SQL column types.
Tested via entity insert + read-back (Table API).

Legend:
- **exact** = lossless round-trip, value identical after insert + read
- **date kept** = date component preserved, time component discarded by the database
- **time kept** = time component preserved, date component discarded by the database
- **lossy** = value stored but precision reduced (e.g. f64 rounding)
- **truncated** = value stored but digits beyond column precision are cut
- **err** = database rejects the insert or entity conversion fails on read

## Chrono Types

### MySQL

[tests/mysql/1_chrono.rs](tests/mysql/1_chrono.rs)

| Column \ Rust type | NaiveDate | NaiveTime | NaiveDateTime | DateTime\<Utc\> |
|---|---|---|---|---|
| VARCHAR | exact | exact | exact | exact |
| DATE | exact | err | date kept | date kept |
| TIME | err | exact | time kept | time kept |
| DATETIME | exact | err | exact | exact |
| TIMESTAMP | exact | err | exact | exact |

Format: `"2025-01-10 12:00:00"` (space separator, no T, no timezone).
`DateTime<Utc>` drops the timezone — MySQL DATETIME doesn't store it,
TIMESTAMP handles UTC conversion internally.
Non-UTC offsets (e.g. `+05:30`) are converted to UTC before storage.

### PostgreSQL

[tests/postgres/1_chrono.rs](tests/postgres/1_chrono.rs)

| Column \ Rust type | NaiveDate | NaiveTime | NaiveDateTime | DateTime\<Utc\> |
|---|---|---|---|---|
| VARCHAR | exact | exact | exact | exact |
| DATE | exact | err | date kept | date kept |
| TIME | err | exact | time kept | time kept |
| TIMESTAMP | exact | err | exact | exact |
| TIMESTAMPTZ | exact | err | exact | exact |

Format: `"2025-01-10 12:00:00+00"` (space separator, abbreviated tz offset).
Typed binds (chrono types, not text) are required for DATE/TIME/TIMESTAMP/TIMESTAMPTZ columns.

### SQLite

[tests/sqlite/1_chrono.rs](tests/sqlite/1_chrono.rs)

| Column \ Rust type | NaiveDate | NaiveTime | NaiveDateTime | DateTime\<Utc\> |
|---|---|---|---|---|
| TEXT | exact | exact | exact | exact |

SQLite stores all dates as TEXT. Format: ISO 8601 with T separator (`"2025-01-10T12:00:00Z"`).
No type coercion — what you write is what you read.

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
| BIGINT | fractional part lost | exact | truncated |

No cross-conversion between Integer, Float, and Decimal CBOR types.
VARCHAR works for all types since `from_cbor` parses text as a fallback.

### PostgreSQL

[tests/postgres/1_decimal.rs](tests/postgres/1_decimal.rs)

| Column \ Rust type | Decimal | i64 | f64 |
|---|---|---|---|
| VARCHAR | exact | exact | exact |
| NUMERIC(20,6) | truncated to 6 places | err | err |
| NUMERIC(38,15) | exact | err | err |
| DOUBLE PRECISION | lossy (~15 digits) | err | exact |
| REAL | lossy (~7 digits) | err | lossy (~7 digits) |
| BIGINT | fractional part lost | exact | truncated |

Typed binds (rust_decimal::Decimal) are used for NUMERIC columns.

### SQLite

[tests/sqlite/1_decimal.rs](tests/sqlite/1_decimal.rs)

| Column \ Rust type | Decimal | i64 | f64 |
|---|---|---|---|
| TEXT | exact | exact | exact |
| NUMERIC | lossy (~15 digits, stored as REAL) | exact | exact |
| REAL | lossy (~15 digits) | err | exact |
| INTEGER | fractional part lost | exact | truncated |

SQLite does not report declared column types on read — the CBOR type is inferred
from the stored value. NUMERIC/REAL affinities coerce to float internally, so
`i64` stored in REAL comes back as Float and fails. Store Decimal in TEXT for lossless precision.
