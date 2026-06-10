# redb panics (not Err) on unsupported search — reachable from generic code

- **Severity:** medium
- **Category:** omissions
- **Location:** `vantage-redb/src/redb/impls/table_source.rs:72`

`Redb::search_table_condition` unconditionally `panic!`s. Because `search_table_condition` is a trait method on `TableSource`, generic UI/table code that offers a search box and calls it on whatever backend is configured will abort the process when that backend happens to be redb. A backend declaring "feature not supported" must do so via the `Result`/error channel the rest of the trait uses, not by panicking — otherwise any code path that can route to redb is a latent crash.

```rust
fn search_table_condition<E>(&self, _table: &Table<Self, E>, _search_value: &str)
    -> Self::Condition {
    panic!("vantage-redb: full-table search is not supported — use indexed eq() instead")
}
```

**Recommendation:** Return a `RedbCondition` that resolves to an error (or change the trait to return `Result`), mirroring how redb's `get_table_sum/max/min` already return `Err(...)` instead of panicking.
