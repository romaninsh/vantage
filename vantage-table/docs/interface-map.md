# Interface Map: TableSource / TableQuerySource / TableExprSource

How the three datasource traits relate, overlap, and what they unlock on `Table<DS, E>`.

## Trait Hierarchy

```text
DataSource                          (marker, no methods)
├── TableSource                     (required — columns, CRUD, aggregation)
│   ├── TableQuerySource            (optional — returns query objects before execution)
│   └── TableExprSource             (optional — returns composable expressions)
└── ExprDataSource<Value>           (execute expressions, defer)
```

`TableSource` is the only required trait. The other two are opt-in and share query-building
internals with `TableSource`.

## Method Cross-Reference

The table below shows how the same underlying query appears across all three traits.

| Operation            | TableSource (execute)                       | TableQuerySource (query object)   | TableExprSource (composable expr)       |
| -------------------- | ------------------------------------------- | --------------------------------- | --------------------------------------- |
| **List rows**        | `list_table_values` → IndexMap              | `get_table_select_query` → Select | —                                       |
| **Get by ID**        | `get_table_value` → Record                  | —                                 | —                                       |
| **Get first**        | `get_table_some_value` → Option             | —                                 | —                                       |
| **Count**            | `get_count` → i64                           | —                                 | `get_table_expr_count` → AssociatedExpr |
| **Sum**              | `get_sum` → Type                            | —                                 | —                                       |
| **Max**              | `get_max` → Type                            | —                                 | `get_table_expr_max` → AssociatedExpr   |
| **Min**              | `get_min` → Type                            | —                                 | `get_table_expr_min` → AssociatedExpr   |
| **Col values**       | `column_values_expression` → AssociatedExpr | —                                 | —                                       |
| **Insert**           | `insert_table_value`                        | —                                 | —                                       |
| **Insert (auto ID)** | `insert_table_return_id_value`              | —                                 | —                                       |
| **Replace**          | `replace_table_value`                       | —                                 | —                                       |
| **Patch**            | `patch_table_value`                         | —                                 | —                                       |
| **Delete**           | `delete_table_value`                        | —                                 | —                                       |
| **Delete all**       | `delete_table_all_values`                   | —                                 | —                                       |
| **Stream**           | `stream_table_values` (default)             | —                                 | —                                       |
| **Search**           | `search_expression`                         | —                                 | —                                       |

### Shared internal: `build_select`

For query-driven backends (SQL, SurrealDB), `list_table_values`, `get_table_select_query`,
`get_table_expr_count`, and `get_count` all start from the same internal logic: build a SELECT from
the table's name, columns, conditions, order_by, and pagination. The difference is only what happens
after building:

```text
build_select(table)
  ├── execute + parse rows    → list_table_values / get_count / get_sum
  ├── return query object     → get_table_select_query
  └── wrap in AssociatedExpr  → get_table_expr_count / get_table_expr_max / column_values_expression
```

Backends should implement a shared `build_select` helper to avoid duplication.

### Gaps / Alignment Opportunities

| Gap                                       | Notes                                                                                                                                                                        |
| ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| No `get_table_expr_sum`                   | `TableExprSource` has `get_table_expr_max` but no sum equivalent. `TableSource` has `get_sum`. Consider adding `get_table_expr_sum` for symmetry.                            |
| No `get_table_expr_column_values`         | `column_values_expression` on `TableSource` already returns `AssociatedExpression`. Could live on `TableExprSource` instead — it's expression-returning, not data-returning. |
| `get_table_select_query` only covers list | No query-object variants for count/sum/insert. `TableQuerySource` could grow `get_table_count_query`, `get_table_insert_query` etc.                                          |
| `search_expression` lives on TableSource  | It returns an `Expression`, not data. Could be on `TableExprSource`, but it needs `TableLike` access for column flags, so `TableSource` is fine.                             |

## What Each Trait Unlocks on `Table<DS, E>`

### TableSource (required)

Implementing `TableSource` for a datasource `DS` automatically gives `Table<DS, E>`:

| Table method / trait impl                             | Delegation                                               | Source                               |
| ----------------------------------------------------- | -------------------------------------------------------- | ------------------------------------ |
| `ReadableValueSet::list_values()`                     | → `list_table_values`                                    | `table/sets/readable_value_set.rs`   |
| `ReadableValueSet::get_value(id)`                     | → `get_table_value`                                      | `table/sets/readable_value_set.rs`   |
| `ReadableValueSet::get_some_value()`                  | → `get_table_some_value`                                 | `table/sets/readable_value_set.rs`   |
| `ReadableValueSet::stream_values()`                   | → `stream_table_values`                                  | `table/sets/readable_value_set.rs`   |
| `ReadableDataSet::list()`                             | → `list_table_values` + entity conversion                | `table/sets/readable_dataset.rs`     |
| `ReadableDataSet::get(id)`                            | → `get_table_value` + entity conversion                  | `table/sets/readable_dataset.rs`     |
| `ReadableDataSet::get_some()`                         | → `get_table_some_value` + entity conversion             | `table/sets/readable_dataset.rs`     |
| `Table::stream()`                                     | → `stream_table_values` + entity conversion              | `table/sets/readable_dataset.rs`     |
| `WritableValueSet::insert_value(id, record)`          | → `insert_table_value`                                   | `table/sets/writable_value_set.rs`   |
| `WritableValueSet::replace_value(id, record)`         | → `replace_table_value`                                  | `table/sets/writable_value_set.rs`   |
| `WritableValueSet::patch_value(id, partial)`          | → `patch_table_value`                                    | `table/sets/writable_value_set.rs`   |
| `WritableValueSet::delete(id)`                        | → `delete_table_value`                                   | `table/sets/writable_value_set.rs`   |
| `WritableValueSet::delete_all()`                      | → `delete_table_all_values`                              | `table/sets/writable_value_set.rs`   |
| `WritableDataSet::insert(id, entity)`                 | → `insert_table_value` + entity conversion               | `table/sets/writable_dataset.rs`     |
| `WritableDataSet::replace(id, entity)`                | → `replace_table_value` + entity conversion              | `table/sets/writable_dataset.rs`     |
| `WritableDataSet::patch(id, partial)`                 | → `patch_table_value` + entity conversion                | `table/sets/writable_dataset.rs`     |
| `WritableDataSet::delete(id)`                         | → `delete_table_value`                                   | `table/sets/writable_dataset.rs`     |
| `WritableDataSet::delete_all()`                       | → `delete_table_all_values`                              | `table/sets/writable_dataset.rs`     |
| `InsertableValueSet::insert_return_id_value(record)`  | → `insert_table_return_id_value`                         | `table/sets/insertable_value_set.rs` |
| `InsertableDataSet::insert_return_id(entity)`         | → `insert_table_return_id_value` + entity conversion     | `table/sets/insertable_dataset.rs`   |
| `TableLike::get_count()`                              | → `get_count`                                            | `table/impls/table_like.rs`          |
| `TableLike::search_expression(value)`                 | → `search_expression`                                    | `table/impls/table_like.rs`          |
| Column management (`with_column`, `get_column`, etc.) | → `create_column`, `to_any_column`, `convert_any_column` | `table/impls/columns.rs`             |
| Expression factory (`Table::expr(...)`)               | → `expr`                                                 | `table/impls/expr.rs`                |

### TableQuerySource (optional)

| Unlocks                                              | Notes                                                                                                                                |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `table.data_source().get_table_select_query(&table)` | Returns a `Select` query object (e.g. `SurrealSelect`) that can be inspected, modified, or passed to other systems before execution. |
| Enables query-aware optimizations in vantage-table   | Table can check if DS implements `TableQuerySource` and use query composition instead of loading all data.                           |

### TableExprSource (optional)

| Unlocks                                                | Notes                                                                                                                 |
| ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------- |
| `table.data_source().get_table_expr_count(&table)`     | Returns `AssociatedExpression` — can be `.get().await` for the value, or composed into another query (e.g. subquery). |
| `table.data_source().get_table_expr_max(&table, &col)` | Same pattern for MAX aggregation.                                                                                     |
| Cross-table subqueries                                 | `AssociatedExpression` can be embedded in conditions of another table (e.g. `WHERE id IN (SELECT id FROM ...)`).      |

## Implementation Status

### SurrealDB

| Trait                            | Status          | Notes                                                    |
| -------------------------------- | --------------- | -------------------------------------------------------- |
| `DataSource`                     | ✅              | Marker trait, impl in `impls/mod.rs`                     |
| `ExprDataSource<AnySurrealType>` | ✅              | `execute()` and `defer()` in `impls/expr_data_source.rs` |
| `TableSource`                    | ⏳ Phase A done | Columns + expr done. Read/write ops are `todo!()`        |
| `SelectableDataSource`           | ❌              | Needed before `TableQuerySource`                         |
| `TableQuerySource`               | ❌              | Needs `SelectableDataSource`                             |
| `TableExprSource`                | ❌              | Needs working `get_count`/`get_sum` first                |

### CSV

| Trait                        | Status | Notes                                                                  |
| ---------------------------- | ------ | ---------------------------------------------------------------------- |
| `DataSource`                 | ✅     | Marker trait                                                           |
| `ExprDataSource<AnyCsvType>` | ✅     | Resolves deferred params; no real query execution                      |
| `TableSource`                | ✅     | Full read impl (in-memory filtering). Write ops return read-only error |
| `SelectableDataSource`       | ❌     | N/A — CSV has no query language                                        |
| `TableQuerySource`           | ❌     | N/A — CSV has no query language                                        |
| `TableExprSource`            | ❌     | Could be added with deferred-fn pattern but low value                  |

### MockTableSource

| Trait                  | Status | Notes                                              |
| ---------------------- | ------ | -------------------------------------------------- |
| `DataSource`           | ✅     | Marker trait                                       |
| `ExprDataSource`       | ✅     | Delegates to configurable `query_source`           |
| `TableSource`          | ✅     | Full CRUD via in-memory `IndexMap` behind `Mutex`  |
| `SelectableDataSource` | ✅     | Returns `MockSelect`, delegates to `select_source` |
| `TableQuerySource`     | ❌     | Not implemented                                    |
| `TableExprSource`      | ✅     | Count + max via in-memory data                     |

### Recommended implementation order (SurrealDB)

1. Implement `build_select` helper (shared query builder from Table state)
2. Implement `TableSource` read ops using `build_select` + execute
3. Implement `SelectableDataSource` (returns `SurrealSelect`)
4. Implement `TableQuerySource` using `build_select` (return without execute)
5. Implement `TableExprSource` using `build_select` + `AssociatedExpression` wrapping
6. Implement `TableSource` write ops (needs insert query builder)

## Naming Recommendations

Current method names mix several conventions. The `_table_` infix is intentional — these methods
take a `&Table` argument and live on a trait called `TableSource`, so the infix disambiguates them
from the `ValueSet`/`DataSet` methods that delegate to them. The `_value`/`_values` suffix is also
intentional — it marks the ValueSet layer (raw `Record<Value>`) as distinct from the DataSet layer
(typed entities). Recommendations below preserve both.

### 1. Consistent `_table_` pattern across all three traits

`TableSource` uses `verb_table_noun` (e.g. `list_table_values`). The other two traits should follow
the same pattern instead of `get_table_expr_*` or `get_table_select_*`.

| Current (TableQuerySource) | Proposed                   | Rationale               |
| -------------------------- | -------------------------- | ----------------------- |
| `get_table_select_query`   | `get_table_select_query` ✓ | Already follows pattern |

| Current (TableExprSource) | Proposed               | Rationale                                  |
| ------------------------- | ---------------------- | ------------------------------------------ |
| `get_table_expr_count`    | `get_table_count_expr` | Noun-last: matches `_table_values` pattern |
| `get_table_expr_max`      | `get_table_max_expr`   | Same reorder: `_table_{what}_{form}`       |

Pattern becomes: `{verb}_table_{what}_{form}` — e.g. `get_table_count_expr`,
`get_table_select_query`.

### 2. Consistent verb for aggregation

Currently: `get_count`, `get_sum` (imperative, executes) vs `get_table_expr_count` (returns expr).
The `get_` prefix is fine for execute-and-return. Expr variants follow the reorder from §1.

| Operation | TableSource (execute) | TableExprSource (expr)                 |
| --------- | --------------------- | -------------------------------------- |
| Count     | `get_count` ✓         | `get_table_count_expr`                 |
| Sum       | `get_sum` ✓           | `get_table_sum_expr` (new, for parity) |
| Max       | `get_max` (new)       | `get_table_max_expr`                   |
| Min       | `get_min` (new)       | `get_table_min_expr` (new)             |

### 3. `column_values_expression` → `column_table_values_expr`

The full word `expression` is used nowhere else — everything else uses `expr`. Also adding `_table_`
since it takes a `&Table` arg, matching the convention.

### 4. `search_expression` → `search_table_expr`

Same two fixes — abbreviate to `expr`, add `_table_` since it takes `&impl TableLike`.

### 5. Summary: before/after

| Current                        | After                          | Changed? |
| ------------------------------ | ------------------------------ | -------- |
| `create_column`                | `create_column`                | —        |
| `to_any_column`                | `to_any_column`                | —        |
| `convert_any_column`           | `convert_any_column`           | —        |
| `expr`                         | `expr`                         | —        |
| `search_expression`            | `search_table_expr`            | ✏️       |
| `list_table_values`            | `list_table_values`            | —        |
| `get_table_value`              | `get_table_value`              | —        |
| `get_table_some_value`         | `get_table_some_value`         | —        |
| `get_count`                    | `get_count`                    | —        |
| `get_sum`                      | `get_sum`                      | —        |
| —                              | `get_max` (new)                | ✏️       |
| —                              | `get_min` (new)                | ✏️       |
| `insert_table_value`           | `insert_table_value`           | —        |
| `replace_table_value`          | `replace_table_value`          | —        |
| `patch_table_value`            | `patch_table_value`            | —        |
| `delete_table_value`           | `delete_table_value`           | —        |
| `delete_table_all_values`      | `delete_table_all_values`      | —        |
| `insert_table_return_id_value` | `insert_table_return_id_value` | —        |
| `stream_table_values`          | `stream_table_values`          | —        |
| `column_values_expression`     | `column_table_values_expr`     | ✏️       |
| `get_table_select_query`       | `get_table_select_query`       | —        |
| `get_table_expr_count`         | `get_table_count_expr`         | ✏️       |
| `get_table_expr_max`           | `get_table_max_expr`           | ✏️       |
| —                              | `get_table_min_expr` (new)     | ✏️       |
