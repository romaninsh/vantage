# MockTableSource::with_data silently drops rows without "id" — count and list disagree

- **Severity:** low
- **Category:** omissions
- **Location:** `vantage-table/src/mocks/mock_table_source.rs:50-77`

`with_data` stores all rows in the `data` HashMap (used by `get_table_count`) but only copies rows that have an `"id"` field into the `ImDataSource` (used by `list_table_values`/`get_table_value`). Seed data without ids is silently dropped from reads while still being counted, so `get_count()` and `list().len()` disagree — a confusing trap for anyone writing tests against the mock. Rows whose id is neither string nor number `panic!` with a leftover `[DEBUG]` message instead of returning an error.

```rust
for value in data.iter() {
    if let Some(id_value) = value.get("id") {
        let id_str = match id_value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => {
                panic!("[DEBUG] ID field is not a string or number: {:?}", id_value);
            }
        };
        ...
    } // else: row silently skipped
}
```

**Recommendation:** Generate an id for id-less rows (the `ImTable::generate_id` helper exists) or panic/error loudly on them, and derive `get_table_count` from the same `ImDataSource` store so the two views can't diverge.
