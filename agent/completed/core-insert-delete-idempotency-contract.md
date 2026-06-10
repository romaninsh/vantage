# insert/delete idempotency contracts contradicted by the framework's own implementations

- **Severity:** high
- **Category:** inconsistencies
- **Location:** `vantage-dataset/src/traits/dataset.rs:179-189` (contract) vs `vantage-table/src/mocks/mock_table_source.rs:273-279,328-331` (violations)

`WritableDataSet::insert` is documented "**Idempotent**: ... If entity already exists, must return success without overwriting data, returning original data", and `WritableValueSet::delete` is documented "**Idempotent**: Always succeeds, even if the record doesn't exist." `ImTable` implements exactly that. But `MockTableSource` (which backs `Table`'s dataset impls) does the opposite — `insert_table_value` errors on an existing id and `delete_table_value` errors on a missing id — and `vantage-table/src/table/sets/writable_dataset.rs:92-99,153-155` has tests *asserting* the error behavior ("Test insert with existing ID should fail", "Test delete non-existing ID should fail"). Callers cannot rely on the documented retry-safety semantics; behavior depends on which backend they happen to use.

```rust
// MockTableSource::insert_table_value — contradicts the trait contract
if im_table.get_value(id).await?.is_some() {
    return Err(vantage_core::error!("Record with ID already exists", id = id));
}
// MockTableSource::delete_table_value — contradicts "always succeeds"
if im_table.get_value(id).await?.is_none() {
    return Err(vantage_core::error!("Record not found", id = id));
}
```

**Recommendation:** Pick one contract per operation and enforce it everywhere: either fix the trait docs (insert fails on duplicate, delete fails on missing) or fix `MockTableSource` and the tests to match the documented idempotent semantics. Driver crates should be audited against the chosen contract.
