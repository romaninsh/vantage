# AWS — DynamoDB Composite-Key Work

# TODO

## Composite-key `get_value` for DynamoDB single-table designs

`DynamoDB::get_table_value(id)` only builds a one-attribute `Key` (partition key), so it can't fetch
items from tables with a composite primary key (HASH + RANGE). The fix is gated on giving `DynamoId`
a sort-key half and teaching `get_table_value` to fill it from the table's pre-set conditions when
present.

### Symptom

A `vantage-ui` page bound to a composite-key DynamoDB table loads the grid fine (Scan returns full
items), but double-clicking a row to open the side sheet fails with:

```
ValidationException: The provided key element does not match the schema
```

Example from a real session (`cp_dynamo_subscriptions.yaml`):

- Table: `ddb-odieplat-mercplat-dev-api`
- Schema: HASH = `PK`, RANGE = `SK`
- YAML pins `PK = "SUBSCRIPTION"` via `params:` and flags `SK` as the id
- Grid passes the SK value to `get_value`; `get_value` sends `{ SK: "USER#…SUB#…" }` as the Key —
  DynamoDB rejects it because the partition key (`PK`) is missing.

### Why the CLI doesn't hit it

`vantage-cli-util::model_cli::run` (used by `examples/dynamo-single-table.rs`) never reaches
`get_value`. For single-record mode (`subscription[0]`, `subscription id=…`) it adds an equality
condition on the id field and calls `get_some_value()`, which does a `Scan` with `FilterExpression`
and returns the first matching item. That sidesteps the key-shape issue but is O(table) per lookup —
fine for a CLI, wrong for a UI sheet that fires on every double-click.

### Current code shape

- `src/dynamodb/id.rs` — `DynamoId(String)` carries only the partition value; its module doc already
  calls out "Composite (partition + sort) keys arrive in a follow-up."
- `src/dynamodb/impls/table_source.rs:40-55` — `id_field_name` reads the one `id_field` configured
  on the table; `key_for_id` builds a one-entry `JsonMap`.
- `src/dynamodb/impls/table_source.rs:163-183` — `get_table_value` calls `key_for_id` directly with
  no awareness of the table's conditions.
- `src/dynamodb/impls/table_source.rs` module header already documents the limitation:
  > Composite-key tables are partially supported: writes carry the full item, but `get_table_value`
  > only knows the partition key (`DynamoId` is partition-only in v0). Sort-key tables work for
  > Scan/Put/Delete when the caller hands in items containing both keys.

### Proposed fix

Two layers:

1. **Widen `DynamoId`** to carry an optional sort-key half:

   ```rust
   pub struct DynamoId {
       pub hash: AttributeValue,        // S or N today
       pub range: Option<AttributeValue>,
   }
   ```

   Keep `From<String>` / `Display` returning the hash-only shape so existing callers don't break;
   add `with_range`, `hash_str`, `range_str` accessors. `from_attr` returns just the hash half (the
   call site at `item_to_record` doesn't know which attribute is the range, so it stays single-half
   there).

2. **Enrich the `Key` map in `get_table_value`** from the table's `add_condition`-set equality
   conditions. When the table has an `eq(field, value)` condition whose field isn't the id field,
   treat the field as the partition-key counterpart and merge it into the `Key` map. For the
   `cp_dynamo_subscriptions.yaml` case this picks up `PK = "SUBSCRIPTION"` automatically.

   Sketch:

   ```rust
   async fn get_table_value(...) {
       let id_field = id_field_name(table);
       let mut key = key_for_id(&id_field, id)?;
       for cond in table.conditions().iter() {
           if let DynamoCondition::Eq { field, value } = cond {
               if *field != id_field {
                   key.insert(field.clone(), attr_to_json(value)?);
               }
           }
       }
       transport::get_item(self.aws(), table.table_name(), key).await
   }
   ```

   This works for the common single-table-design pattern (`PK = constant, SK = row id`) without
   forcing every caller to construct composite `DynamoId`s. Tables with two genuinely-dynamic key
   halves still need the wider `DynamoId` to round-trip the second half explicitly — call that out
   in the function doc.

### Tests to add

`tests/dynamodb_live.rs` only covers a partition-key-only table (`vantage-demo-products`,
`with_id_column("id")`). Add a second fixture that:

- Provisions a composite-key table (HASH = `PK`, RANGE = `SK`).
- Inserts an item with `PK = "TYPE#X"`, `SK = "ID#abc"`.
- Builds the table with `with_id_column("SK")` + a static `add_condition(eq("PK", "TYPE#X"))`.
- Calls `get_value("ID#abc")` and asserts the item comes back.

### Workaround until then

UI code that needs the double-click sheet on a composite-key DynamoDB table can fall back to
rendering whatever the grid already has loaded in memory (the row payload is the same shape as
`GetItem` would return) rather than re-fetching by id. Tracked separately in `vantage-ui`.
