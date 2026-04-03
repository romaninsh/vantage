# Vantage-SurrealDB 0.3 Upgrade Plan

## TableSource Implementation

### Phase B: Read Operations (in progress)

`build_select` helper in `surrealdb/impls/build_select.rs` constructs `SurrealSelect` from Table
state (source, columns, conditions, ordering). Pagination TODO (needs access without `TableLike`'s
`'static` bound).

**API change:** `get_sum`/`get_max`/`get_min` now take `Column<AnyType>` and return
`Result<Self::Value>` instead of generic `Result<Type>`. Updated across all backends.

| Method                 | SurrealDB Query                               | Status |
| ---------------------- | --------------------------------------------- | ------ |
| `get_sum`              | `RETURN math::sum(SELECT VALUE col FROM ...)` | todo   |
| `get_max`              | `RETURN math::max(SELECT VALUE col FROM ...)` | todo   |
| `get_min`              | `RETURN math::min(SELECT VALUE col FROM ...)` | todo   |
| `list_table_values`    | SurrealSelect → execute → parse CBOR rows     | todo   |
| `get_table_value`      | SELECT \* FROM ONLY table:id                  | todo   |
| `get_table_some_value` | SELECT \* FROM table LIMIT 1                  | todo   |

### Phase C: Write Operations

Depends on porting `insert/mod.rs` to use `surreal_expr!`.

| Method                         | SurrealDB Query             | Status  |
| ------------------------------ | --------------------------- | ------- |
| `insert_table_value`           | CREATE table:id SET ...     | `todo!` |
| `replace_table_value`          | UPDATE table:id CONTENT ... | `todo!` |
| `patch_table_value`            | UPDATE table:id MERGE ...   | `todo!` |
| `delete_table_value`           | DELETE table:id             | `todo!` |
| `delete_table_all_values`      | DELETE table                | `todo!` |
| `insert_table_return_id_value` | CREATE ... RETURN id        | `todo!` |
| `column_table_values_expr`     | SurrealSelect VALUE col     | `todo!` |

### Additional traits (optional, unlocks more features)

All three traits share `build_select` internally. See `vantage-table/docs/interface-map.md` for full
cross-reference.

- **`SelectableDataSource`** — `select()` returns `SurrealSelect`. Already gives `Table::select()`,
  `get_count_query()`, `get_sum_query()`.
- **`TableQuerySource`** — `get_table_select_query()` returns a `SurrealSelect` for a table.
- **`TableExprSource`** — `get_table_count_expr()` / `get_table_max_expr()` / `get_table_min_expr()`
  / `get_table_sum_expr()` return `AssociatedExpression` for cross-table subqueries.

### Future: Mutation query objects

Currently `select()` returns a query object but write ops execute directly via `TableSource`.
Eventually, `SelectableDataSource` (or a new trait) could add `type Delete`, `type Update`,
`type Insert`, enabling `Table::delete_query()` / `update_query()` that share conditions with
`select()` via `build_select`. Low priority — mutation query inspection is rarely needed.

### Key decision: Record conversion

SurrealDB returns CBOR maps. Converting to `Record<AnySurrealType>` requires:

1. Parse CBOR `Map([(Text(k), v), ...])` → `IndexMap<String, AnySurrealType>`
2. This already works via `IndexMap::<String, AnySurrealType>::from_cbor()`
3. Wrap in `Record::from_indexmap()`

The `id` field in results needs special handling — it's a `Thing` (CBOR Tag 8), not a plain string.
`list_table_values` must extract IDs and return `IndexMap<Thing, Record<AnySurrealType>>`.

---

## Disabled Modules — Status & Plan

| Module              | Status      | Plan                                                                                                                                                                                            |
| ------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `select/`           | ✅ enabled  | Done, all tests pass                                                                                                                                                                            |
| `surrealdb/impls/`  | ✅ partial  | `base.rs` + `expr_data_source.rs` + `table_source.rs` (Phase A done, Phase B/C are `todo!()`)                                                                                                   |
| `identifier`        | ✅ enabled  | Done                                                                                                                                                                                            |
| `operation`         | ✅ enabled  | Done, updated to use `Expressive`                                                                                                                                                               |
| `thing`             | ✅ enabled  | Done                                                                                                                                                                                            |
| `types/`            | ✅ enabled  | Done, Display + Expressive impls added                                                                                                                                                          |
| `sum` / `macros`    | ✅ enabled  | Done                                                                                                                                                                                            |
| `surreal_return`    | ✅ enabled  | Done                                                                                                                                                                                            |
| `column`            | ❌ disabled | Uses old 0.2 types (`surreal_client::types::SurrealType`, `TypeInfo`). Probably not needed — use `Column<Type>` from vantage-table directly (like CSV does).                                    |
| `typed_expression`  | ❌ disabled | Uses old `Expr` / `IntoExpressive` from 0.2. The new `Expressive<AnySurrealType>` + `RefOperation` covers most use cases. May revive later for compile-time type checking on column operations. |
| `conditional`       | ❌ disabled | IF-THEN-ELSE builder. Uses old `Expression` (JSON). Trivial to port — just switch to `surreal_expr!`. Low priority.                                                                             |
| `associated_query`  | ❌ disabled | Old query association pattern. Replaced by `AssociatedExpression` from vantage-expressions.                                                                                                     |
| `field_projection`  | ❌ disabled | Old field projection. Replaced by `SelectField` + `Field`.                                                                                                                                      |
| `protocol`          | ❌ disabled | Old trait definitions. Replaced by `Selectable` / `ExprDataSource` from vantage-expressions.                                                                                                    |
| `variable`          | ❌ disabled | SurrealDB LET variable support. Low priority, can be added later.                                                                                                                               |
| `insert/`           | ❌ disabled | Insert query builder. Needed for TableSource write operations. Port to use `surreal_expr!`.                                                                                                     |
| `table/`            | ❌ disabled | Old SurrealTableExt. Will be rebuilt after TableSource is done.                                                                                                                                 |
| `selectsource`      | ❌ disabled | Old `SelectSource` impl. Uses JSON. Replace with `SelectableDataSource` or integrate into TableSource.                                                                                          |
| `tablesource` (old) | ❌ disabled | 0.2 impl with 24 methods. Reference for new implementation but can't be reused directly.                                                                                                        |
| `prelude`           | ❌ disabled | Re-exports. Re-enable once public API stabilizes.                                                                                                                                               |

### Disabled tests

| Test file                  | Status      | Depends on                                                                                                                    |
| -------------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `select.rs`                | ✅ enabled  | —                                                                                                                             |
| `return.rs`                | ✅ enabled  | live SurrealDB                                                                                                                |
| `types.rs`                 | ✅ enabled  | live SurrealDB                                                                                                                |
| `queries.rs`               | ❌ disabled | `prelude`, `SurrealMockBuilder`, `Table::into_entity`, `select_surreal` — needs TableSource + table extensions                |
| `search_expression.rs`     | ❌ disabled | `TableSource::search_table_expr` — enable after search impl uses column flags                                                 |
| `table_ext.rs`             | ❌ disabled | SurrealTableExt — enable after table extensions                                                                               |
| `table_ext_mocked.rs`      | ❌ disabled | Same                                                                                                                          |
| `test_expressions.rs`      | ❌ disabled | Old `surreal_client::types` API, `SurrealExpression`, `IntoExpression` — **delete**, functionality covered by new type system |
| `test_insert_cbor.rs`      | ❌ disabled | Insert query builder                                                                                                          |
| `test_insert_unstructured` | ❌ disabled | Insert query builder                                                                                                          |

---

## Crates to Examine for Next Steps

### Must read (directly involved)

- **`vantage-table/src/traits/table_source.rs`** — the trait to implement. Read the full trait
  definition including all method signatures, associated types, and doc comments.
- **`vantage-table/src/traits/table_query_source.rs`** — optional trait for query-aware backends.
  Read to understand how `SurrealSelect` integrates with the table layer.
- **`vantage-table/src/traits/table_expr_source.rs`** — optional trait for expression-aware
  backends. Read to understand `AssociatedExpression` integration.
- **`vantage-table/src/table/`** — the `Table<DS, E>` struct. Understand how it delegates to
  `TableSource` methods, how columns/conditions/sorting are stored.
- **`vantage-csv/src/table_source.rs`** — reference implementation. Shows exactly how each
  `TableSource` method is implemented for a simple backend. The SurrealDB version follows the same
  pattern but builds queries instead of reading files.
- **`vantage-surrealdb/src/surrealdb/tablesource.rs`** — old 0.2 implementation. Useful as reference
  for SurrealDB-specific query patterns (how to build SELECT/INSERT/UPDATE/DELETE), but the trait
  signatures are outdated.

### Must read (type system integration)

- **`vantage-table/src/column/`** — `Column<Type>` and `ColumnType` trait. Understand how columns
  work with generic types, type erasure via `to_any_column`.
- **`vantage-types/src/lib.rs`** — `Entity`, `Record<T>`, `EmptyEntity`. Understand how entities map
  to/from records.
- **`vantage-table/src/traits/column_like.rs`** — `ColumnLike` trait that columns must implement.

### Should skim (context)

- **`surreal-client/src/client.rs`** — `query_cbor()` method, understand what it sends/receives.
- **`vantage-table/src/traits/table_like.rs`** — dyn-safe trait, understand what TableSource enables
  at the `TableLike` level.
- **`vantage-dataset/`** — `ReadableValueSet`, `WritableValueSet`, `InsertableValueSet` traits.
  These are what `Table<DS, E>` implements using `TableSource` methods.
- **`vantage-table/tests/`** — integration tests, especially any CSV-based tests that exercise the
  full Table → TableSource → DataSource chain.
- **`vantage-cli-util/`** — consumes `Table` via `TableLike`. Understanding its expectations helps
  verify the implementation is complete.
- **`bakery_model3/`** — example model, currently uses SurrealDB but with many features commented
  out. Will be the integration test once TableSource is done.

### Not needed yet

- `vantage-mongodb/` — separate database backend, independent work.
- `vantage-live/` — caching layer, builds on top of TableSource.
- `vantage-ui-adapters/` — UI integration, orthogonal.
- `vantage-config/` — YAML-based table definitions, works at a higher level.

---

## Rough Order of Work

1. **Finish Phase B** — `get_sum`, `get_max`, `get_min`, then `list_table_values`,
   `get_table_value`, `get_table_some_value` (needs CBOR → Record conversion).
2. **Implement `SelectableDataSource`** — returns `SurrealSelect` from `select()`.
3. **Implement `TableQuerySource`** — `get_table_select_query()` using `build_select`.
4. **Implement `TableExprSource`** — `get_table_count_expr` / `get_table_sum_expr` /
   `get_table_max_expr` / `get_table_min_expr` using `build_select` + `AssociatedExpression`.
5. **Port `insert/mod.rs`** — needed for write operations in TableSource.
6. **Phase C: Write operations** — `insert_table_value`, `replace_table_value`, etc.
7. **Enable `search_expression.rs` test** — once search impl uses column flags.
8. **Delete `test_expressions.rs`** — old 0.2 API, fully superseded.
9. **Rebuild table extensions** — SurrealDB-specific methods on `Table<SurrealDB, E>`.
10. **Enable remaining tests** one by one, updating to 0.3 API.
11. **Update `bakery_model3`** — uncomment SurrealDB integration, verify full stack works.
