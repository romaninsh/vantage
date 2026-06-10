# README DataSet/Pagination examples use method names that don't exist

- **Severity:** high
- **Category:** inconsistencies
- **Location:** `README.md:241-283` (also `README.md:314`)

The "DataSet operations" and "Type casting" sections call `load_some()`, `delete_id(&id)`, `insert_id(id, client)`, `get_id_as::<T>(id)` and `get_id_value(id)`. None of these methods exist in `vantage-dataset` — the actual trait (`vantage-dataset/src/traits/dataset.rs` and `valueset.rs`) exposes `get_some()`, `get(id)`, `insert(&id, &entity)`, `delete(&id)`, `replace`, `patch`, `insert_return_id`. Likewise `Pagination::ipp(25)` (README.md:314) does not exist; `vantage-table/src/pagination.rs` only has `Pagination::new(page, items_per_page)` and `set_ipp`. Copy-pasting any of these snippets fails to compile.

```
// Basic loading operation - load one client record
let (id, client) = clients.load_some().await?;
// Next - delete it from memory
clients.delete_id(&id).await?;
// Insert it back
clients.insert_id(id, client).await?;
...
order.set_pagination(Pagination::ipp(25));
```

**Recommendation:** Update the snippets to the real API (`get_some`, `delete`, `insert`, `get_as`/value accessors as they actually exist, `Pagination::new(0, 25)`), ideally by extracting them from a compiled example (the `learn-*` crates pattern used by the book).
