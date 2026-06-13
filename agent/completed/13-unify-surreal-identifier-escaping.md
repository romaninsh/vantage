# Unify the two divergent SurrealDB identifier-escaping implementations

- **Severity:** low
- **Category:** suggestions
- **Location:** `vantage-surrealdb/src/identifier.rs:49`, `surreal-client/src/record.rs:353`

There are two independent SurrealDB identifier escapers with different rigor. `surreal-client::record::escape_identifier` is the careful one: it escapes empty, numeric, leading-digit, and any non-`[A-Za-z0-9_]` identifier with `⟨…⟩` and replaces embedded `⟩` with `\⟩`. `vantage-surrealdb::identifier::Identifier::expr` is the weak one: a space/keyword allowlist and no embedded-`⟩` escaping (see the related security finding). Having two implementations guarantees they drift, and the weaker one is the one used to build queries.

```rust
// surreal-client (good):
return format!("⟨{}⟩", ident.replace('⟩', "\\⟩"));
// vantage-surrealdb (weak):
Expression::new(format!("⟨{}⟩", self.identifier), vec![])
```

**Recommendation:** Extract one escaping function (the `surreal-client` version) into a shared location and have both `Identifier` and `Thing` rendering use it, so escaping rules live in exactly one place.
