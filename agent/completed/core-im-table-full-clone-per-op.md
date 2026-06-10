# Every ImTable operation clones the entire table

- **Severity:** medium
- **Category:** performance
- **Location:** `vantage-dataset/src/im/mod.rs:50-53`

`get_or_create_table` deep-clones the whole `IndexMap<String, Record<V>>` (every row, every value) on *every* operation — including single-row reads like `get_value` and `get_some_value`, and writes additionally write the full clone back via `update_table`. A point lookup is O(table size) in time and memory instead of O(1). `ImDataSource` is the backing store for `MockTableSource` and in-memory datasets, so test suites and any in-memory production use pay quadratic cost as tables grow (n operations × n rows).

```rust
pub(super) fn get_or_create_table(&self, table_name: &str) -> IndexMap<String, Record<V>> {
    let mut tables = self.tables.lock().unwrap();
    tables.entry(table_name.to_string()).or_default().clone()
}
```

**Recommendation:** Operate on the map in place under the lock (closure-based accessor like `with_table(&self, name, |t| ...)`), cloning only the rows actually returned. This also fixes the lost-update race reported separately.
