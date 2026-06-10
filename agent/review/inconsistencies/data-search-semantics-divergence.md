# `search_table_condition` has wildly different semantics per engine

- **Severity:** medium
- **Category:** inconsistencies
- **Location:** `vantage-sql/src/sqlite/impls/table_source.rs:110`, `vantage-surrealdb/src/surrealdb/impls/table_source.rs:122`, `vantage-mongodb/src/mongodb/impls/table_source.rs:115`, `vantage-csv/src/table_source.rs:65`, `vantage-redb/src/redb/impls/table_source.rs:64`

The same trait method behaves incompatibly across the six data engines, so identical UI search input yields different results (or failures) depending on backend:

- SQLite/Postgres/MySQL: `LIKE %v% ESCAPE '$'` over all columns, case-sensitivity backend-dependent (Postgres casts `::text`, others don't).
- SurrealDB: `string::contains(string::lowercase(...))` — case-insensitive, no wildcard escaping needed.
- MongoDB: case-insensitive `$regex` with metacharacter escaping.
- CSV: returns a 0-param `SEARCH '...'` expression that is silently ignored downstream (returns all rows).
- redb: `panic!("full-table search is not supported")` — a panic, not a `Result::Err`.

**Recommendation:** Define the contract (case sensitivity, which columns, empty-table result) in the trait doc and conform each backend; redb and CSV should return `Result::Err`, never panic or silently no-op.
