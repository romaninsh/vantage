# SurrealDB Identifier escaping is incomplete and bypassable

- **Status:** FIXED (2026-06-13). Two parts:
  - The keyword-allowlist bypass was closed earlier (#297, commit `e9d44ae4`): the allowlist is gone
    and a single `surreal-client::escape_identifier` authority now quotes anything outside
    `[A-Za-z0-9_]` / leading digit / reserved keyword / empty.
  - That fix's *escaping* was itself broken and injectable on SurrealDB 3.x — it emitted `\⟩` for an
    embedded `⟩`, but `\⟩` is an invalid escape inside `⟨…⟩` (verified live), and backslashes weren't
    doubled, so a crafted `\⟩` collapsed and closed the quoting early (live-verified injection running
    a smuggled `RETURN 999`). Fixed in surreal-client 0.5.2 / vantage-surrealdb 0.5.12: backslash →
    `\\` first, then `⟩` → `\u{27E9}`. Regression tests: `record::tests::test_escape_identifier`
    (surreal-client) and `identifier::tests::{embedded_close_bracket_cannot_break_out_of_quoting,
    crafted_backslash_bracket_cannot_break_out_of_quoting}` (vantage-surrealdb).
- **Related regression found while fixing (NOT addressed here):** #297's quote-everything rule also
  wraps the `$parent` *variable* (`Parent::identifier()`) as `⟨$parent⟩`, which is a literal field
  name, not the parent-row variable — `vantage-surrealdb` test `select::query11` fails on the base
  branch because of this. Variable-like identifiers (`$…`) should bypass quoting. Tracked separately.
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
