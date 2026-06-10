# get_table_count_expr calls Handle::block_on inside the runtime — guaranteed panic

- **Severity:** medium
- **Category:** bugs
- **Location:** `vantage-table/src/mocks/mock_table_source.rs:492-520`

`TableExprSource::get_table_count_expr` is a sync method that grabs the current Tokio handle with `Handle::try_current()` and immediately calls `handle.block_on(...)`. `Handle::block_on` panics with "Cannot block the current thread from within a runtime" when invoked from a runtime worker thread — which is exactly the case in which `try_current()` succeeds. So whenever this is called from async code (the normal case for table operations), it panics; when called outside a runtime it silently returns 0. A TODO in the code acknowledges this, and the only test exercising the path is commented out.

```rust
let count = tokio::runtime::Handle::try_current()
    .map(|handle| {
        // TODO: we shouldn't use block_on here
        handle.block_on(async {
            self.data.lock().await.get(table_name).map(|data| data.len()).unwrap_or(0)
        })
    })
    .unwrap_or_else(|_| 0);
```

**Recommendation:** Replace `tokio::sync::Mutex` with `std::sync::Mutex` for `data` (no await is needed under the lock) so the count can be read synchronously, or register the mock answer lazily via a `DeferredFn` instead of pre-computing it.
