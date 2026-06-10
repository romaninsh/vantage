# vantage-sql Identifier does not escape embedded quote characters

- **Severity:** medium
- **Category:** security
- **Location:** `vantage-sql/src/primitives/identifier.rs:67`

`Identifier::render_with` wraps each part in the backend quote char but never doubles an embedded quote. `ident("a\"b")` renders `"a"b"` on Postgres/SQLite (and the backtick variant on MySQL), breaking out of the quoted identifier. The doc comment explicitly warns "do not pass untrusted user input," yet the Rhai engine exposes `ident()`, `table()`, and `from(name)` taking arbitrary config strings (`rhai_engine/constructors.rs`, `select_methods.rs`), so config-as-code authored by a less-trusted source can inject. The correct, cheap fix is to double the quote — the standard SQL identifier escaping.

```rust
fn render_with(&self, q: char) -> String {
    let base = self.parts.iter()
        .map(|p| format!("{q}{p}{q}"))  // embedded `q` not doubled
        .collect::<Vec<_>>().join(".");
    // …
}
```

**Recommendation:** Escape by doubling: `format!("{q}{}{q}", p.replace(q, &format!("{q}{q}")))`. Reject identifiers containing NUL.
