# CSV backend re-reads and re-parses the whole file on every operation

- **Severity:** low
- **Category:** performance
- **Location:** `vantage-csv/src/table_source.rs:93`, `vantage-csv/src/csv.rs:53`

Every `TableSource` read on the CSV backend calls `read_csv`, which opens the file from disk and parses all rows into an `IndexMap` from scratch. `get_table_value` parses the entire file just to `records.get(id)`; `get_table_count` parses everything to call `.len()`; `get_some_value` parses everything to take `.next()`. For a related-records resolution that calls `get_value` per id, this is O(rows × lookups) full re-parses. There is no caching of the parsed table even within a single logical operation.

```rust
async fn get_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<Option<Record<Self::Value>>> {
    let records = self.read_csv(table.table_name(), table.columns())?; // full parse
    Ok(records.get(id).cloned())
}
```

**Recommendation:** Cache the parsed table (keyed by path + mtime) behind the `Csv` handle, or at least reuse one parse across a batch operation; for `get_table_value`, stop scanning once the id is found.
