# Flatten::resolve_deferred is a documented no-op — flatten() silently skips deferred parameters

- **Severity:** medium
- **Category:** omissions
- **Location:** `vantage-expressions/src/expression/flatten.rs:65-70`

The `Flatten` trait promises "Flatten an expression by resolving all deferred parameters and nested expressions", and the module docs sell flattening as the step that makes parameter binding safe. But the only implementation, `ExpressionFlattener::resolve_deferred`, just clones the expression and leaves `ExpressiveEnum::Deferred` parameters untouched (an internal comment admits it is for testing). Any consumer that calls `flattener.flatten(&expr)` and then binds parameters will pass unresolved `DeferredFn` values through, with no error or warning — the API contract and behavior disagree, and being sync, this method can never fulfil the contract for async deferreds.

```rust
fn resolve_deferred(&self, expr: &Expression<T>) -> Expression<T> {
    // Note: This is a sync implementation that doesn't actually execute deferred closures
    // For testing purposes, deferred parameters are left as-is
    // In real usage, this would be handled by the DataSource execute method
    expr.clone()
}
```

**Recommendation:** Remove `resolve_deferred` from the trait (flattening and deferred resolution are different phases — resolution is async and belongs to the DataSource), or make `flatten` return an error when deferred parameters remain.
