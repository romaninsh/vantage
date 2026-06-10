# SQL backends disagree on placeholder/param-count validation

- **Severity:** medium
- **Category:** inconsistencies
- **Location:** `vantage-sql/src/sqlite/impls/expr_data_source.rs:89`, `vantage-sql/src/postgres/impls/expr_data_source.rs:108`, `vantage-sql/src/mysql/expr_data_source.rs:104`

The three `prepare_typed_query` implementations are near-identical but handle a placeholder/parameter mismatch three different ways. Postgres and MySQL `assert_eq!` the counts — a mismatch **panics** the whole process (a query-builder bug becomes a hard crash / DoS). SQLite has no assertion: on mismatch it silently emits trailing `?N` placeholders with no separator, producing malformed SQL that fails at the driver instead. Same trait, same invariant, three behaviors (panic vs. silent corruption).

```rust
// postgres / mysql:
assert_eq!(template_parts.len(), flattened.parameters.len() + 1, /* … */);
// sqlite: no such check — relies on the driver to reject the bad SQL
```

**Recommendation:** Pick one strategy for all three — return a `Result::Err` (not `assert!`) on mismatch so a builder bug surfaces as a recoverable error rather than a panic or corrupt SQL.
