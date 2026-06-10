# ImDataSource read-modify-write race loses concurrent updates

- **Severity:** high
- **Category:** bugs
- **Location:** `vantage-dataset/src/im/mod.rs:50-58`

Every `ImTable` operation calls `get_or_create_table` (which clones the whole table out of the mutex and releases the lock), mutates the clone, then calls `update_table` (which re-acquires the lock and overwrites the whole table). Two concurrent writers on the same table will each clone the same snapshot and the second `update_table` silently discards the first writer's changes. The idempotency check in `insert_value`/`insert` (`if table.get(id).is_some()`) is also race-prone: two concurrent inserts of the same id can both succeed and the later one overwrites. `ImDataSource` is `Clone` over an `Arc` and the API is async, so concurrent use is the expected mode.

```rust
pub(super) fn get_or_create_table(&self, table_name: &str) -> IndexMap<String, Record<V>> {
    let mut tables = self.tables.lock().unwrap();
    tables.entry(table_name.to_string()).or_default().clone()
}

pub(super) fn update_table(&self, table_name: &str, table: IndexMap<String, Record<V>>) {
    let mut tables = self.tables.lock().unwrap();
    tables.insert(table_name.to_string(), table);
}
```

**Recommendation:** Mutate in place under a single lock acquisition (e.g. expose `with_table_mut(&self, name, impl FnOnce(&mut IndexMap<...>) -> R)`), so each operation is atomic with respect to the shared map.
