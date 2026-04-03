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
| **Count**            | `get_count` → i64                           | —                                 | `get_table_count_expr` → AssociatedExpr |
| **Sum**              | `get_sum` → Type                            | —                                 | `get_table_sum_expr` → AssociatedExpr   |
| **Max**              | `get_max` → Type                            | —                                 | `get_table_max_expr` → AssociatedExpr   |
| **Min**              | `get_min` → Type                            | —                                 | `get_table_min_expr` → AssociatedExpr   |
| **Col values**       | `column_table_values_expr` → AssociatedExpr | —                                 | —                                       |
| **Insert**           | `insert_table_value`                        | —                                 | —                                       |
| **Insert (auto ID)** | `insert_table_return_id_value`              | —                                 | —                                       |
| **Replace**          | `replace_table_value`                       | —                                 | —                                       |
| **Patch**            | `patch_table_value`                         | —                                 | —                                       |
| **Delete**           | `delete_table_value`                        | —                                 | —                                       |
| **Delete all**       | `delete_table_all_values`                   | —                                 | —                                       |
| **Stream**           | `stream_table_values` (default)             | —                                 | —                                       |
| **Search**           | `search_table_expr`                         | —                                 | —                                       |

### Shared internal: `build_select`

For query-driven backends (SQL, SurrealDB), `list_table_values`, `get_table_select_query`,
`get_table_count_expr`, and `get_count` all start from the same internal logic: build a SELECT from
the table's name, columns, conditions, order_by, and pagination. The difference is only what happens
after building:

```text
build_select(table)
  ├── execute + parse rows    → list_table_values / get_count / get_sum
  ├── return query object     → get_table_select_query
  └── wrap in AssociatedExpr  → get_table_count_expr / get_table_max_expr / column_table_values_expr
```

Backends should implement a shared `build_select` helper to avoid duplication.

### Gaps / Alignment Opportunities

| Gap                                       | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `column_table_values_expr` on TableSource | Returns `AssociatedExpression`, not data. Could live on `TableExprSource` instead — it's expression-returning, not data-returning.                                                                                                                                                                                                                                                                                                                                                 |
| `get_table_select_query` only covers list | No query-object variants for count/sum/insert. `TableQuerySource` could grow `get_table_count_query`, `get_table_insert_query` etc.                                                                                                                                                                                                                                                                                                                                                |
| `search_table_expr` lives on TableSource  | It returns an `Expression`, not data. Could be on `TableExprSource`, but it needs `TableLike` access for column flags, so `TableSource` is fine.                                                                                                                                                                                                                                                                                                                                   |
| No mutation query objects                 | `select()` returns a query object, but write ops execute directly via `TableSource`. Future: add `type Delete`, `type Update`, `type Insert` to `SelectableDataSource` (or a new `MutableDataSource` trait), enabling `delete_query()`, `update_query()` on Table. `delete_query()` and `update_query()` would share conditions/table name with `select()` via `build_select`. `insert_query()` shares only table name. Low priority — mutation query inspection is rarely needed. |

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
| `TableLike::search_expression(value)`                 | → `search_table_expr`                                    | `table/impls/table_like.rs`          |
| Column management (`with_column`, `get_column`, etc.) | → `create_column`, `to_any_column`, `convert_any_column` | `table/impls/columns.rs`             |
| Expression factory (`Table::expr(...)`)               | → `expr`                                                 | `table/impls/expr.rs`                |

### SelectableDataSource (optional, required by TableQuerySource)

When `DS` also implements `SelectableDataSource`, Table gets query-building convenience methods:

| Table method                 | Delegation / behaviour                                  | Source                      |
| ---------------------------- | ------------------------------------------------------- | --------------------------- |
| `Table::select()`            | Builds `DS::Select` with columns/conditions/order/page  | `table/impls/selectable.rs` |
| `Table::get_count()`         | → `get_count` (execute)                                 | `table/impls/selectable.rs` |
| `Table::get_sum(&col)`       | → `get_sum` (execute)                                   | `table/impls/selectable.rs` |
| `Table::get_max(&col)`       | → `get_max` (execute)                                   | `table/impls/selectable.rs` |
| `Table::get_min(&col)`       | → `get_min` (execute)                                   | `table/impls/selectable.rs` |
| `Table::get_count_query()`   | `select().as_count()` — returns expression, no execute  | `table/impls/selectable.rs` |
| `Table::get_sum_query(&col)` | `select().as_sum(col)` — returns expression, no execute | `table/impls/selectable.rs` |

### TableQuerySource (optional)

Requires `SelectableDataSource`. All methods above are available, plus:

| Table method / access                                | Notes                                                                                                                                   |
| ---------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `table.data_source().get_table_select_query(&table)` | Backend-specific query builder. Returns `Result<DS::Select>` — can be inspected, modified, or passed to other systems before execution. |
| Enables query-aware optimizations in vantage-table   | Table can check if DS implements `TableQuerySource` and use query composition instead of loading all data.                              |

Note: `Table::select()` (from `SelectableDataSource`) builds the query generically from table state.
`get_table_select_query()` lets the backend build it with vendor-specific logic.

### TableExprSource (optional)

| Table method                | Delegation             | Notes                                                                                                         |
| --------------------------- | ---------------------- | ------------------------------------------------------------------------------------------------------------- |
| `Table::get_expr_count()`   | `get_table_count_expr` | Returns `AssociatedExpression` — `.get().await` for the value, or compose into another query (e.g. subquery). |
| `Table::get_expr_sum(&col)` | `get_table_sum_expr`   | Same pattern for SUM aggregation.                                                                             |
| `Table::get_expr_max(&col)` | `get_table_max_expr`   | Same pattern for MAX aggregation.                                                                             |
| `Table::get_expr_min(&col)` | `get_table_min_expr`   | Same pattern for MIN aggregation.                                                                             |
| Cross-table subqueries      | —                      | `AssociatedExpression` can be embedded in conditions of another table (e.g. `WHERE id IN (SELECT ...)`).      |

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

## Naming Conventions (applied)

The `_table_` infix is intentional — these methods take a `&Table` argument and live on a trait
called `TableSource`, so the infix disambiguates them from the `ValueSet`/`DataSet` methods that
delegate to them. The `_value`/`_values` suffix marks the ValueSet layer (raw `Record<Value>`) as
distinct from the DataSet layer (typed entities).

Pattern: `{verb}_table_{what}_{form}` — e.g. `get_table_count_expr`, `get_table_select_query`.

### Aggregation methods

| Operation | TableSource (execute) | TableExprSource (expr) |
| --------- | --------------------- | ---------------------- |
| Count     | `get_count`           | `get_table_count_expr` |
| Sum       | `get_sum`             | `get_table_sum_expr`   |
| Max       | `get_max`             | `get_table_max_expr`   |
| Min       | `get_min`             | `get_table_min_expr`   |
