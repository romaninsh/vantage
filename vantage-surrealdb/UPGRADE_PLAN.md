# Vantage-SurrealDB 0.3 Upgrade Plan

## Remaining Work

1. **Implement `SelectableDataSource`** — `select()` returns `SurrealSelect`. Unlocks
   `Table::select()`, `get_count_query()`, `get_sum_query()`.
2. **Implement `TableQuerySource`** — `get_table_select_query()` using `build_select`.
3. **Implement `TableExprSource`** — count/sum/max/min expr methods using `AssociatedExpression`.
4. **Enable `search_expression.rs` test** — once search impl uses column flags.
5. **Delete `test_expressions.rs`** — old 0.2 API, fully superseded.
6. **Rebuild table extensions** — SurrealDB-specific methods on `Table<SurrealDB, E>`.
7. **Enable remaining tests** one by one, updating to 0.3 API.
8. **Pagination support** — wire up limit/skip in `build_select` (needs `TableLike` `'static` fix).

### Future: Mutation query objects

Currently `select()` returns a query object but write ops execute directly via `TableSource`.
Eventually, `SelectableDataSource` (or a new trait) could add `type Delete`, `type Update`,
`type Insert`, enabling `Table::delete_query()` / `update_query()` that share conditions with
`select()` via `build_select`. Low priority — mutation query inspection is rarely needed.

---

## Disabled Modules

| Module              | Plan                                                                    |
| ------------------- | ----------------------------------------------------------------------- |
| `column`            | Not needed — use `Column<Type>` from vantage-table directly.            |
| `typed_expression`  | May revive later for compile-time type checking.                        |
| `conditional`       | IF-THEN-ELSE builder. Trivial to port — just switch to `surreal_expr!`. |
| `associated_query`  | Replaced by `AssociatedExpression` from vantage-expressions.            |
| `field_projection`  | Replaced by `SelectField` + `Field`.                                    |
| `protocol`          | Replaced by `Selectable` / `ExprDataSource` from vantage-expressions.   |
| `variable`          | SurrealDB LET variable support. Low priority.                           |
| `table/`            | Old SurrealTableExt. Rebuild as table extensions.                       |
| `selectsource`      | Replace with `SelectableDataSource` or integrate into TableSource.      |
| `tablesource` (old) | 0.2 impl. Reference only.                                               |
| `prelude`           | Re-enable once public API stabilizes.                                   |

### Disabled tests

| Test file                  | Depends on                                                                 |
| -------------------------- | -------------------------------------------------------------------------- |
| `queries.rs`               | `prelude`, `SurrealMockBuilder`, `select_surreal` — needs table extensions |
| `search_expression.rs`     | `TableSource::search_table_expr` — needs column flags                      |
| `table_ext.rs`             | SurrealTableExt — enable after table extensions                            |
| `table_ext_mocked.rs`      | Same                                                                       |
| `test_expressions.rs`      | Old API — **delete**, functionality covered by new type system             |
| `test_insert_cbor.rs`      | Old insert tests, superseded by `statements.rs`                            |
| `test_insert_unstructured` | Old insert tests, superseded by `statements.rs`                            |

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
- Statement builders live in `src/statements/` — `select/`, `insert/`, `update/`, `delete.rs`. All
  share consistent builder API: `.with_field()`, `.with_any_field()`, `.with_record()`,
  `.with_condition()`. Re-exported at crate root and via backwards-compat `crate::select` etc.
- `Thing` has `Display` impl (`table:id`), `table()`/`id()` accessors, and `FromStr`.
- SurrealDB `CREATE` returns array-wrapped result — `extract_first_map()` helper handles both
  array-of-maps and bare map responses for all write operations.
- Global `OnceLock<SurrealDB>` doesn't work with test harnesses (each `#[tokio::test]` gets its own
  runtime, WS connection dies between tests). Create fresh connection per test instead.
- `connect_surrealdb()` in bakery_model3 uses `cbor://` DSN scheme, not `ws://`.
- Re-exported `CborValue` and `SurrealConnection` from vantage-surrealdb for downstream use.
