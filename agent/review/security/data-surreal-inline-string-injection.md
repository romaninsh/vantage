# SurrealQL injection in inline single-quoted primitives (similarity / time_group)

- **Status:** FIXED (2026-06-13, vantage-surrealdb 0.5.11) — both `similarity` and `time_group`
  now bind their literal token as a `Scalar` parameter (the recommended fix), routed through
  `prepare_query` → CBOR `$_arg`, so a quote can no longer break out of the literal. Verified
  against a live SurrealDB: the payload `x') OR true OR ('` bypassed the filter inline but is
  contained once bound. Regression tests: `primitives::tests::tier2_literal_tokens_cannot_break_out_of_query`
  and `tier2_literal_tokens_are_bound_params`.
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
