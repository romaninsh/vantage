# Table::select_column unwraps column lookup — panics on unknown field name

- **Severity:** medium
- **Category:** bugs
- **Location:** `vantage-table/src/table/impls/selectable.rs:135`

`select_column(&self, field: &str)` calls `self.get_column_expr(field).unwrap()`. The field name is a plain string that in Vantage UI typically originates from YAML/Rhai config, so a typo in a config file panics the process instead of surfacing a config error. The sibling lookup APIs (`column()`, `get_ref_as`, `lookup_ref`) all return `Option`/`Result` for the same situation, so this is also internally inconsistent.

```rust
pub fn select_column(&self, field: &str) -> Expression<T::Value>
where
    T::Column<T::AnyType>: Expressive<T::Value>,
    T::Select: Expressive<T::Value>,
{
    let expr = self.get_column_expr(field).unwrap();
    let mut select = self.select_empty();
    ...
}
```

**Recommendation:** Return `Result<Expression<T::Value>>` and propagate a "column not found" `VantageError` (with the table and field in context), matching `lookup_ref`'s behavior.
