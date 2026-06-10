# unimplemented!() panics reachable through normal TableSource API

- **Severity:** medium
- **Category:** omissions
- **Location:** `vantage-api-pool/src/pool_api.rs:371`

`PoolApi::related_in_condition` and `PoolApi::column_table_values_expr` are `unimplemented!()`, and `RestApi::column_table_values_expr` (`vantage-api-client/src/rest/table_source.rs:377`) likewise. These are ordinary `TableSource` trait methods invoked by relation traversal (`with_many`/`with_one` style references); on these backends a config that declares a reference panics the process instead of returning an error. Every other backend in scope (vantage-cmd, vantage-aws) returns a structured `error!(...)` for unsupported operations — these are the only hard panics on the trait surface.

```rust
fn related_in_condition<SourceE: Entity<Self::Value> + 'static>(
    &self,
    _target_field: &str,
    _source_table: &Table<Self, SourceE>,
    _source_column: &str,
) -> Self::Condition
{
    unimplemented!("related_in_condition not yet supported for API pool")
}
```

**Recommendation:** Return a sentinel/error-bearing condition (or restructure the trait to allow `Result`) so unsupported traversal surfaces as a recoverable error like the read-only write paths do, rather than aborting an admin-console session.
