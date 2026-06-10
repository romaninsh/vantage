# SurrealDB Identifier escaping is incomplete and bypassable

- **Severity:** high
- **Category:** security
- **Location:** `vantage-surrealdb/src/identifier.rs:49`

`Identifier::needs_escaping` only flags identifiers that contain a space or match a hard-coded 11-word keyword list. Identifiers containing other SurrealQL-significant characters (`:`, `-`, `(`, `;`, `.`, leading digits, etc.) are emitted **raw** into the query. Worse, even when it does escape it wraps with `⟨{}⟩` without escaping an embedded `⟩`, so an identifier containing `⟩` breaks out of the brackets. Identifiers flow from column names / Rhai `ident()`/`table()` config strings into query text (they render as `Nested`, never parameterized), so a crafted field name injects SurrealQL.

```rust
fn needs_escaping(&self) -> bool {
    let reserved_keywords = ["DEFINE","CREATE","SELECT", /* …11 total… */ "TABLE"];
    self.identifier.contains(' ') || reserved_keywords.contains(&upper.as_str())
}
// expr(): Expression::new(format!("⟨{}⟩", self.identifier), vec![])  // embedded ⟩ not escaped
```

**Recommendation:** Always escape identifiers using `⟨…⟩` with `⟩` → `\⟩` replacement (as `surreal-client::record::escape_identifier` already does), instead of a keyword allowlist. Share one implementation across the crates.
