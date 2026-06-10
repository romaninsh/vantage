# RestApi search condition is silently dropped — search returns all rows

- **Severity:** medium
- **Category:** bugs
- **Location:** `vantage-api-client/src/rest/table_source.rs:103`

`search_table_condition` builds an expression with template `SEARCH '<value>'`, but the only consumer of conditions, `condition_to_query_param` (`rest/operation.rs:38`), requires template `"{} = {}"` and returns `None` otherwise. The search condition is therefore never sent as a query param and never applied client-side — a user typing a search in the UI gets the full, unfiltered result set presented as if every row matched. The same dead `SEARCH '...'` shape exists in `vantage-api-pool/src/pool_api.rs:132`.

```rust
fn search_table_condition<E>(
    &self,
    _table: &Table<Self, E>,
    search_value: &str,
) -> Expression<Self::Value>
{
    Expression::new(format!("SEARCH '{}'", search_value), vec![])
}
```

**Recommendation:** Either implement search (map to a configurable query param like `q`/`search`, or filter rows in memory across columns) or return an error/`Unsupported` so the UI can disable the search box instead of showing wrong results.
