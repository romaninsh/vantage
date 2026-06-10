# search_table_condition embeds parameter placeholder inside a quoted SQL literal

- **Severity:** medium
- **Category:** security
- **Location:** `vantage-table/src/mocks/mock_table_source.rs:159-168`

The reference implementation of `TableSource::search_table_condition` builds `name LIKE '%{}%'` — the `{}` parameter placeholder sits *inside* a single-quoted string literal. Real parameter binding cannot bind into the middle of a literal, so any driver copying this canonical pattern either produces broken SQL or falls back to string interpolation of the raw user search value (which is what `preview()`-based execution does in the mocks) — a textbook SQL/LIKE injection shape. The search value also isn't escaped for LIKE metacharacters (`%`, `_`).

```rust
fn search_table_condition<E>(
    &self,
    _table: &Table<Self, E>,
    search_value: &str,
) -> Expression<Self::Value>
where
    E: Entity<Self::Value>,
{
    expr_any!("name LIKE '%{}%'", search_value)
}
```

**Recommendation:** Bind the whole pattern as one parameter: build `format!("%{}%", escape_like(search_value))` as the scalar and use `expr_any!("name LIKE {}", pattern)`. Audit driver crates for copies of this template.
