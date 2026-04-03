# Vantage-SurrealDB 0.3 Upgrade Plan

## TableSource Implementation

### Phase B: Read Operations ✅

`build_select` helper in `surrealdb/impls/build_select.rs` constructs `SurrealSelect` from Table
state (source, columns, conditions, ordering). Pagination TODO (needs access without `TableLike`'s
`'static` bound).

All read + aggregation methods implemented and tested against live DB with entity round-trips.

| Method                 | SurrealDB Query                               | Status |
| ---------------------- | --------------------------------------------- | ------ |
| `get_count`            | `RETURN count(SELECT VALUE id FROM ...)`      | ✅     |
| `get_sum`              | `RETURN math::sum(SELECT VALUE col FROM ...)` | ✅     |
| `get_max`              | `RETURN math::max(SELECT VALUE col FROM ...)` | ✅     |
| `get_min`              | `RETURN math::min(SELECT VALUE col FROM ...)` | ✅     |
| `list_table_values`    | SurrealSelect → execute → parse CBOR rows     | ✅     |
| `get_table_value`      | SELECT \* FROM ONLY table:id                  | ✅     |
| `get_table_some_value` | SELECT \* FROM table LIMIT 1                  | ✅     |

Record conversion: `parse_cbor_row()` helper extracts `Thing` IDs using `table.id_field()` (falls
back to `"id"`), converts CBOR maps to `Record<AnySurrealType>`. Full entity round-trip verified (DB
→ Record → `Product::from_record()` etc).

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

---

## Disabled Modules — Status & Plan

| Module              | Status      | Plan                                                                                      |
| ------------------- | ----------- | ----------------------------------------------------------------------------------------- |
| `select/`           | ✅ enabled  | Done, all tests pass                                                                      |
| `surrealdb/impls/`  | ✅ enabled  | `base.rs` + `expr_data_source.rs` + `table_source.rs` (Phase A+B done, Phase C `todo!()`) |
| `identifier`        | ✅ enabled  | Done                                                                                      |
| `operation`         | ✅ enabled  | Done, updated to use `Expressive`                                                         |
| `thing`             | ✅ enabled  | Done                                                                                      |
| `types/`            | ✅ enabled  | Done, Display + Expressive impls added                                                    |
| `sum` / `macros`    | ✅ enabled  | Done                                                                                      |
| `surreal_return`    | ✅ enabled  | Done                                                                                      |
| `column`            | ❌ disabled | Uses old 0.2 types. Not needed — use `Column<Type>` from vantage-table directly.          |
| `typed_expression`  | ❌ disabled | Uses old `Expr` / `IntoExpressive`. May revive later for compile-time type checking.      |
| `conditional`       | ❌ disabled | IF-THEN-ELSE builder. Trivial to port — just switch to `surreal_expr!`. Low priority.     |
| `associated_query`  | ❌ disabled | Replaced by `AssociatedExpression` from vantage-expressions.                              |
| `field_projection`  | ❌ disabled | Replaced by `SelectField` + `Field`.                                                      |
| `protocol`          | ❌ disabled | Replaced by `Selectable` / `ExprDataSource` from vantage-expressions.                     |
| `variable`          | ❌ disabled | SurrealDB LET variable support. Low priority.                                             |
| `insert/`           | ❌ disabled | Insert query builder. Needed for Phase C. Port to use `surreal_expr!`.                    |
| `table/`            | ❌ disabled | Old SurrealTableExt. Will be rebuilt after Phase C.                                       |
| `selectsource`      | ❌ disabled | Replace with `SelectableDataSource` or integrate into TableSource.                        |
| `tablesource` (old) | ❌ disabled | 0.2 impl. Reference only.                                                                 |
| `prelude`           | ❌ disabled | Re-enable once public API stabilizes.                                                     |

### Disabled tests

| Test file                  | Status      | Depends on                                                                 |
| -------------------------- | ----------- | -------------------------------------------------------------------------- |
| `select.rs`                | ✅ enabled  | —                                                                          |
| `return.rs`                | ✅ enabled  | live SurrealDB                                                             |
| `types.rs`                 | ✅ enabled  | live SurrealDB                                                             |
| `table_source_read.rs`     | ✅ enabled  | live SurrealDB, bakery_model3 entities                                     |
| `queries.rs`               | ❌ disabled | `prelude`, `SurrealMockBuilder`, `select_surreal` — needs table extensions |
| `search_expression.rs`     | ❌ disabled | `TableSource::search_table_expr` — needs column flags                      |
| `table_ext.rs`             | ❌ disabled | SurrealTableExt — enable after table extensions                            |
| `table_ext_mocked.rs`      | ❌ disabled | Same                                                                       |
| `test_expressions.rs`      | ❌ disabled | Old API — **delete**, functionality covered by new type system             |
| `test_insert_cbor.rs`      | ❌ disabled | Insert query builder (Phase C)                                             |
| `test_insert_unstructured` | ❌ disabled | Insert query builder (Phase C)                                             |

---

## Remaining Work

1. **Implement `SelectableDataSource`** — returns `SurrealSelect` from `select()`.
2. **Implement `TableQuerySource`** — `get_table_select_query()` using `build_select`.
3. **Implement `TableExprSource`** — count/sum/max/min expr methods using `AssociatedExpression`.
4. **Port `insert/mod.rs`** — needed for write operations in TableSource.
5. **Phase C: Write operations** — `insert_table_value`, `replace_table_value`, etc.
6. **Enable `search_expression.rs` test** — once search impl uses column flags.
7. **Delete `test_expressions.rs`** — old 0.2 API, fully superseded.
8. **Rebuild table extensions** — SurrealDB-specific methods on `Table<SurrealDB, E>`.
9. **Enable remaining tests** one by one, updating to 0.3 API.
10. **Pagination support** — wire up limit/skip in `build_select` (needs `TableLike` `'static` fix).

---

## Notes & Learnings

- `into_entity()` is not needed when return type is annotated — `Table::new` infers `E` from
  context. Removed from all bakery_model3 table constructors.
- `#[entity()]` macro now supports multiple type systems: `#[entity(CsvType, SurrealType)]`. Option
  fields handled gracefully — tries `try_get::<Option<T>>()` first, falls back to
  `try_get::<T>().map(Some)`, missing fields → `None`.
- Entity structs can diverge between CSV and SurrealDB table definitions — same struct, different
  `with_column` calls. CSV-only fields (like `bakery_id`, `client_id`) made `Option` so they don't
  break SurrealDB deserialization.
- `SurrealType` for custom enums (like `Animal`) is trivial — store as CBOR Text, parse back.
- Global `OnceLock<SurrealDB>` doesn't work with test harnesses (each `#[tokio::test]` gets its own
  runtime, WS connection dies between tests). Create fresh connection per test instead.
- `connect_surrealdb()` in bakery_model3 uses `cbor://` DSN scheme, not `ws://`.
- Re-exported `CborValue` and `SurrealConnection` from vantage-surrealdb for downstream use.
