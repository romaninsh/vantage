# vantage-config.schema.json is the old-prototype schema; doesn't match what code parses

- **Severity:** high
- **Category:** inconsistencies
- **Location:** `vantage-config.schema.json:1`

The schema shipped at the repo root is byte-identical to `vantage-ui-old-prototype/vantage-config.schema.json` (last touched 2025-10-30, "Rewrote README for 0.3"). The current parser — `vantage-ui/crates/inventory/src/config.rs` (`Inventory`) and `table.rs` (`TableConfig`), whose `bin/generate-schema.rs` is the designated generator for this exact filename — accepts `datasource`, `id_column`, `expressions`, `params`, `graphql`, `has_many`, `references`, `surreal`, `cmd`, and VistaSpec map-style columns. None of these appear in the schema; instead the schema's `EntityConfig` requires `table` and `columns` (array-only) which the code no longer requires. Anyone validating YAML against this schema gets false errors on every modern config and no completion for real fields.

```
"EntityConfig": { ... "required": ["columns", "table"], ... }
// vs vantage-ui/crates/inventory/src/table.rs:
pub struct TableConfig {
    pub name: Option<String>,
    pub datasource: Option<String>,
    ...
    pub expressions: HashMap<String, ExpressionDef>,
    pub references: IndexMap<String, ReferenceFull>,
    pub surreal: Option<SurrealDriverBlock>,
    pub cmd: Option<CmdDriverBlock>,
}
```

**Recommendation:** Regenerate the file with `vantage-ui/crates/inventory/bin/generate-schema.rs` (which writes `vantage-config.schema.json` from the live `Inventory` struct) and add that step to CI, or delete the stale copy from this repo if the schema now lives with vantage-ui.
