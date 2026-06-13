# CSV search filter is silently a no-op (returns all rows)

- **Severity:** medium
- **Category:** bugs
- **Location:** `vantage-csv/src/table_source.rs:65`

`Csv::search_table_condition` returns `Expression::new(format!("SEARCH '{}'", search_value), vec![])` — a template with **zero parameters**. When that condition reaches `apply_condition`, the first guard `if params.len() < 2 { return Ok(records); }` (`condition.rs:26`) matches, so the records pass through **unfiltered**. A user searching a CSV-backed table silently gets every row instead of the matching subset — a correctness and data-exposure bug (the UI presents it as a filtered result). The interpolated, unescaped `'{}'` is also dead/misleading code.

```rust
fn search_table_condition<E>(&self, _table: &Table<Self, E>, search_value: &str)
    -> Expression<Self::Value> {
    Expression::new(format!("SEARCH '{}'", search_value), vec![])  // 0 params → ignored downstream
}
```

**Recommendation:** Either implement a real substring filter over the table's columns (matching the SQL/Surreal/Mongo backends) or return an explicit "search unsupported" error so the no-op cannot masquerade as a filtered result.
