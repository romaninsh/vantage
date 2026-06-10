# Stale source files outside the module tree (would not compile if re-enabled)

- **Severity:** medium
- **Category:** omissions
- **Location:** `vantage-table/src/with_columns.rs:1` (also `vantage-table/src/operation.rs`, `vantage-table/src/record.rs`, `vantage-table/src/column/any.rs`, `vantage-dataset/src/datasetsource.rs`)

Several `.rs` files ship in the crates' src directories but are not declared in any `mod` tree, so they are silently skipped by the compiler and have rotted: `with_columns.rs` references a nonexistent `crate::with_ordering` module and uses unimported types; `record.rs` imports `vantage_dataset::dataset` (the module is now `traits`) and `crate::Entity` (not exported); `column/any.rs` is commented out in `column/mod.rs` and imports `crate::column::column`; `datasetsource.rs` is commented out in `vantage-dataset/src/lib.rs` and imports `vantage_core::Entity`, which is itself commented out in `vantage-core/src/lib.rs:14-23`. These mislead readers/contributors (and AI tools) about the real API and will produce confusing breakage if anyone re-enables them.

```rust
// vantage-table/src/lib.rs declares: traits, mocks, cbor_ext, conditions,
// pagination, prelude, references, sorting, column, source, table
// — but src/ also contains with_columns.rs, operation.rs, record.rs:
use crate::with_ordering::OrderByExt;          // with_columns.rs test — module doesn't exist
use vantage_dataset::dataset::{Result, WritableDataSet}; // record.rs — module renamed to `traits`
use vantage_core::Entity;                       // datasetsource.rs — Entity is commented out
```

**Recommendation:** Delete the dead files (git history preserves them) or move them under a feature-gated/experimental module that is actually compiled in CI.
