# SurrealDB get_table_value ignores the table's id field name

- **Severity:** low
- **Category:** bugs
- **Location:** `vantage-surrealdb/src/surrealdb/impls/table_source.rs:216`

`get_table_value` hard-codes the literal `"id"` when parsing the returned row, while the sibling methods `list_table_values` and `get_table_some_value` correctly derive `id_field_name` from `table.id_field()`. For a table whose id column is named anything other than `id`, `parse_cbor_row` will fail to extract the `Thing` (it is discarded anyway here, so the record still returns), but the divergence is a latent inconsistency: the same table source resolves the id field three different ways across three read paths.

```rust
let (_thing, record) = parse_cbor_row(map, "id");  // should be table.id_field() name
Ok(Some(record))
```

**Recommendation:** Compute `id_field_name` from `table.id_field()` as the other read methods do, and pass it to `parse_cbor_row` for consistency.
