# SurrealQL injection in inline single-quoted primitives (similarity / time_group)

- **Severity:** high
- **Category:** security
- **Location:** `vantage-surrealdb/src/primitives.rs:219`, `vantage-surrealdb/src/primitives.rs:210`

`similarity(expr, term)` and `time_group(expr, unit)` build the query by interpolating the caller string directly inside a single-quoted SurrealQL literal with no escaping. A `term`/`unit` containing a `'` closes the literal and injects arbitrary SurrealQL. Both are exposed to the Rhai config layer (`rhai_engine/constructors.rs: fn_similarity`/`fn_time_group`), and `similarity` is exactly the kind of helper fed a runtime user search term.

```rust
pub fn similarity(expr: impl Expressive<AnySurrealType>, term: &str) -> Expr {
    Expression::new(
        format!("string::similarity::jaro_winkler({{}}, '{term}')"),  // term unescaped
        vec![ExpressiveEnum::Nested(expr.expr())],
    )
}
```

**Recommendation:** Pass `term`/`unit` as a bound `Scalar` parameter (`{}` placeholder) instead of inlining, or at minimum escape `'` → `\'`. Same applies to `time_group`'s `unit`.
