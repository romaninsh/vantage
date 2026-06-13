# SurrealQL injection via Thing/record-id rendered into query text

- **Status:** DISMISSED (2026-06-13) — no live attacker-controlled path. `Thing` ids
  originate from internal construction (relationship traversal, typed record ids), not
  from raw untrusted user input reaching `Thing::expr()`. The escaping divergence that
  *did* matter (`Identifier`) is already fixed via the shared `escape_identifier`
  (PR #297 / `data-surreal-identifier-escaping.md`). Re-open only if a code path is
  added that builds a `Thing` directly from unvalidated external input.
- **Severity:** critical
- **Category:** security
- **Location:** `vantage-surrealdb/src/thing.rs:132`, `vantage-surrealdb/src/surrealdb/impls/table_source.rs:199`

`Thing::expr()` renders a record id straight into the SurrealQL template with no escaping or parameterization. The query preparer only parameterizes `Scalar` params; `Nested` expressions (which is what `Thing` and `Identifier` become) are flattened directly into the query string (`surrealdb/impls/base.rs:34-52`). So `get_table_value` builds `SELECT * FROM ONLY <table>:<id>` from caller-supplied id. In an admin console that fetches a record by a user-supplied id, an id like `user:1 OR true` (parsed into `Thing{table:"user", id:"1 OR true"}`) is concatenated verbatim, allowing query manipulation / data exfiltration.

```rust
impl Expressive<AnySurrealType> for Thing {
    fn expr(&self) -> Expression<AnySurrealType> {
        surreal_expr!(format!("{}:{}", self.table, self.id))  // no escaping
    }
}
// table_source.rs
let query = crate::surreal_expr!("SELECT * FROM ONLY {}", (id.clone()));
```

**Recommendation:** Bind record ids as real CBOR `$param` values (Tag(8) record-id), or escape both table and id with the `⟨…⟩` form (reuse `surreal-client`'s `escape_identifier`). Never interpolate untrusted ids into the template.
