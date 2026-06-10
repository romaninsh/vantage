# DONE: unify id-parameter convention across dataset/valueset traits

- **Origin finding:** `core-id-param-conventions-and-doc-drift` (severity: low)
- **Status:** DONE — all trait signatures, implementors, and call sites updated.

## What was changed

### Trait signatures (vantage-dataset)

- `ReadableValueSet::get_value` — `&Self::Id` → `impl Into<Self::Id> + Send`
- `WritableValueSet::insert_value/replace_value/patch_value/delete` — same
- `WritableDataSet::insert/replace/patch` — same
- `ActiveRecordSet::get_value_record` — same
- `ActiveEntitySet::get_entity` — same

### Implementors updated

- `vantage-dataset/src/im/{dataset_writable,valueset_writable,valueset_readable}.rs` (ImTable)
- `vantage-dataset/src/mocks/csv.rs` (CsvFile, Id=usize)
- `vantage-table/src/table/sets/{writable_dataset,writable_value_set,readable_value_set}.rs`
- `vantage-vista/src/impls/{writable_value_set,readable_value_set}.rs`
- `vantage-mongodb/src/vista/source.rs`
- `vantage-surrealdb/src/vista/source.rs`

### Internal ref-sites

- `vantage-dataset/src/record.rs` — `ActiveEntity::save/delete`, `ActiveRecord::save`
- `vantage-table/src/table/impls/refereces.rs:~183` — writeback closure

### Call sites swept

- `learn-3/src/vantage_axum.rs`
- `bakery_model3/examples/{dynamo-smoke,contained-surreal}.rs`
- `bakery_model4/examples/cli4.rs`
- `vantage-mongodb/tests/{1_table_source,4_readable_data_set,4_editable_data_set,6_vista}.rs`
- `vantage-aws/tests/{dynamodb_live,vista}.rs`
- `vantage-diorama/src/dio/{mod,worker}.rs`
- `vantage-diorama/src/lens/chunk_sink.rs`
- `vantage-diorama/examples/write_through.rs`

### Not touched (intentionally)

- `vantage-diorama/src/lens/cache_backend.rs` — `CacheTable` is a separate trait from ValueSet,
  takes `&str`
- `vantage-surrealdb/src/table/writable.rs` — pre-migration API (insert_id/replace_id/etc.), not yet
  using standard WritableDataSet trait methods
