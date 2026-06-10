# MySQL JSON_TABLE inlines name/type/path without escaping

- **Severity:** medium
- **Category:** security
- **Location:** `vantage-sql/src/mysql/statements/primitives/json_table.rs:57`

`JsonTableColumn::render` carefully escapes the `DEFAULT '...'` literal (`def.replace('\'', "''")`) but interpolates `name`, `col_type`, and the JSON `path` (which is itself inside single quotes `PATH '{}'`) with no escaping at all. The same applies to `JsonTable`'s `root_path` (`json_table.rs:130`). A `path` or `root_path` containing `'` closes the literal and injects SQL. The inconsistency (one field escaped, the adjacent quoted field not) shows the escaping was an afterthought.

```rust
let mut s = format!("{} {} PATH '{}'", self.name, self.col_type, self.path); // path unescaped
if let Some(ref def) = self.default {
    s.push_str(&format!(" DEFAULT '{}' ", def.replace('\'', "''")));         // but this is escaped
}
```

**Recommendation:** Escape `'` in `path`/`root_path` and validate `name`/`col_type` against an identifier/type allowlist before rendering.
