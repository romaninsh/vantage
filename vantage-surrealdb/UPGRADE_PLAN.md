# Vantage-SurrealDB 0.3 Upgrade Plan

## Remaining Work

1. **Enable remaining tests** one by one, updating to 0.3 API.

### Future: Mutation query objects

Currently `select()` returns a query object but write ops execute directly via `TableSource`.
Eventually, `SelectableDataSource` (or a new trait) could add `type Delete`, `type Update`,
`type Insert`, enabling `Table::delete_query()` / `update_query()` that share conditions with
`select()` via `build_select`. Low priority — mutation query inspection is rarely needed.

---

## Disabled Modules

| Module             | Plan                                                                    |
| ------------------ | ----------------------------------------------------------------------- |
| `typed_expression` | May revive later for compile-time type checking.                        |
| `conditional`      | IF-THEN-ELSE builder. Trivial to port — just switch to `surreal_expr!`. |
| `variable`         | SurrealDB LET variable support. Low priority.                           |
| `prelude`          | Re-enable once public API stabilizes.                                   |

### Superseded modules (kept as reference)

| Module             | Replaced by                                                  |
| ------------------ | ------------------------------------------------------------ |
| `column`           | `Column<Type>` from vantage-table.                           |
| `associated_query` | `AssociatedExpression` from vantage-expressions.             |
| `field_projection` | `SelectField` + `Field`.                                     |
| `protocol`         | `Selectable` / `ExprDataSource` from vantage-expressions.    |
| `table/`           | `SurrealTableExt` in `src/ext.rs` + `impls/table_source.rs`. |
| `selectsource`     | `SelectableDataSource` impl in `impls/`.                     |
| `tablesource`      | `impls/table_source.rs`.                                     |

### Disabled tests

| Test file                  | Status                                                     |
| -------------------------- | ---------------------------------------------------------- |
| `queries.rs`               | Old 0.2 API (`prelude`, `SurrealMockBuilder`). Superseded. |
| `table_ext.rs`             | Old 0.2 `SurrealTableExt`. Superseded by `ext.rs`.         |
| `table_ext_mocked.rs`      | Same.                                                      |
| `test_expressions.rs`      | Old 0.2 expression API. Fully superseded.                  |
| `test_insert_cbor.rs`      | Old insert tests. Superseded by `statements.rs`.           |
| `test_insert_unstructured` | Old insert tests. Superseded by `statements.rs`.           |

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
- `TableExprSource` methods don't need `defer()` — same-database expressions compose directly.
  Deferral is only for cross-database scenarios.
- `Selectable` trait `as_count`/`as_sum` return bare expressions without subquery context. Use
  inherent `SurrealSelect<Rows>` methods for full query wrapping, then `.expr()` via `Expressive`
  trait.
- `Table::select()` (from vantage-table `selectable.rs`) already wires source, columns, conditions,
  ordering, and pagination into `T::Select`. No need for a duplicate `select_surreal()` method — ext
  trait only adds type-narrowing methods (`select_first`, `select_column`, `select_single`).
