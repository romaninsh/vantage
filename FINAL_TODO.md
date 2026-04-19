# FINAL_TODO.md — Features missing in 0.4 that existed in legacy vantage

This file catalogues capabilities that existed in the legacy vantage stack
(`vantage/` source tree + `docs/` + `docs2/`) but have not yet been ported to
the 0.4 workspace (`vantage-core`, `vantage-table`, `vantage-dataset`,
`vantage-sql`, etc.).

Items already tracked in `TODO.md` are not duplicated here — see
[§ Tracked elsewhere](#tracked-elsewhere) for cross-references.

# Dataset trait surface

The 0.4 `ReadableDataSet` / `WritableDataSet` / `InsertableDataSet` traits only
expose `list`, `get`, `get_some`, `insert`, `replace`, `patch`,
`insert_return_id`. Legacy had a richer surface for ad-hoc queries and
DTO-style projections.

- [ ] **Untyped reads: `get_all_untyped()`, `get_row_untyped()`, `get_col_untyped()`,
      `get_one_untyped()`** — fetch as `Vec<Record<Value>>` / `Record<Value>` /
      `Vec<Value>` / `Value` without defining an entity struct. Useful for admin
      tools, scripts, CLI dumps, and generic dataset inspection. Legacy:
      `vantage/src/dataset/readable.rs`. 0.4 can approximate via
      `ReadableValueSet`, but the entity-typed traits lack an escape hatch.
- [ ] **Alternative-type reads: `get_as<T>()` / `get_some_as<T>()`** — fetch into a
      different (usually narrower) serde type than the table's entity. Legacy:
      `vantage/src/dataset/readable.rs`. Used for projections/DTOs where a full
      entity is overkill. 0.4 forces you to either change the table's entity
      type or round-trip through `Value`.
- [ ] **`ScalarDataSet::enumerate()`** — fetch a single column across all rows as
      `Vec<Value>`. Legacy: `vantage/src/dataset/scalar.rs`. The idiomatic way
      to pull IDs, codes, or any one-column listing; 0.4 requires constructing
      a dedicated entity or reaching into backend-specific select builders.
- [ ] **`save_into(other_table)`** — persist an entity into a *different* table,
      e.g. for archival, copy-on-write, or audit-log patterns. Legacy:
      documented in `docs2/src/4.4-writing-records.md`.
- [ ] **Closure-based bulk update: `update<F>(closure)` and `update_with<T>(values)`**
      — legacy `WritableDataSet` could update all matching rows via a closure
      or a serializable patch struct. 0.4 only offers per-row `replace()` /
      `patch()`, which forces a fetch-modify-save loop for bulk edits.

# Table-level query building (abstraction leak)

In legacy, joins/group-by/having/union/CTE/distinct were methods on `Table`
itself, so backend-agnostic code worked against them. In 0.4 they live inside
per-backend `SelectableDataSource` impls (`vantage-sql/src/{mysql,postgres,sqlite}/statements/select/**`),
which means generic code (including `vantage-table`) can't compose them.

- [ ] **Table-level joins** — `with_join()`, `add_join()`, `join_table()` on
      `Table<T, E>`, with automatic alias generation, field merging with table
      prefixing, self-joins, and `with_imported_fields()` for eagerly pulling
      named columns from the joined table into the parent's result set. Legacy:
      `vantage/src/sql/table/with_joins.rs`, `with_columns.rs`,
      `extensions/`. 0.4 has no table-level join API at all — joins are only
      reachable via per-backend `SurrealSelect` / `SqliteSelect` /
      `PostgresSelect` / `MysqlSelect`. Related TODO.md entries cover
      alias-clash and condition preservation bugs but presuppose the API exists.
- [ ] **Table-level `with_group_by()` / `add_group_by()` / `with_having_condition()`**
      — legacy exposed GROUP BY/HAVING at the Table abstraction; 0.4's group-by
      is per-backend (`vantage-sql/src/mysql/.../selectable.rs` etc). TODO.md
      mentions group-by as "someday maybe"; this item is the broader point that
      it needs to live on `Table`, not just inside SQL select builders.
- [ ] **Table-level `with_union()` / compound queries** — the `Union` primitive
      already exists in `vantage-sql/src/primitives/union.rs` (supports UNION,
      UNION ALL, EXCEPT, INTERSECT) but is not surfaced on `Table`. Legacy let
      you union datasets backend-agnostically (docs2 §5.3).
- [ ] **CTE (`add_with`) and DISTINCT at the Table level** — legacy
      `Query::add_with()` and `set_distinct()` were reachable through `Table`;
      0.4 pushes both into backend-specific select builders.

# Hooks / extensions framework

Completely absent from 0.4. A grep for
`TableExtension|before_select_query|before_delete_query|Hook` returns only
legacy and `_archive/` hits. This blocks a whole class of cross-cutting
features (soft delete, audit trail, row-level security, multi-tenancy).

- [ ] **`TableExtension` trait with lifecycle hooks** — `init()`,
      `before_select_query(query)`, `before_delete_query(query)`, attached via
      `table.with_extension(...)`. Legacy:
      `vantage/src/sql/table/extensions/mod.rs`. Hook collector supported
      multiple extensions per table with ordered execution and error
      propagation.
- [ ] **SoftDelete extension** — marks rows as deleted via a flag column,
      auto-injects `is_deleted = false` into SELECTs, converts `DELETE` to
      `UPDATE`. Legacy:
      `vantage/src/sql/table/extensions/soft_delete.rs`. The usage pattern is
      preserved in `bakery_model/examples/1-soft-delete.rs`, but the framework
      support is gone.
- [ ] **Per-operation hooks** — `before_insert`, `after_insert`,
      `before_update`, `after_update`, `before_delete`, `after_delete`.
      Documented in `docs2/src/4.7-operation-hooks.md`. Needed for audit logs,
      denormalised counters, timestamp auto-population, etc.

# Lazy expressions / post-fetch transforms

0.4 has `DeferredFn` (in `vantage-expressions`), but it's only for async
expression flattening during query assembly, not for transforming fetched rows.

- [ ] **`LazyExpression<T, E>` with `BeforeQuery` and `AfterQuery` variants** —
      `BeforeQuery(fn(&Table) -> Expression)` builds a sub-expression with
      access to sibling columns or joined tables; `AfterQuery(fn(Value) -> Value)`
      transforms the fetched value post-query. Legacy:
      `vantage/src/lazy_expression.rs`. The AfterQuery variant is what enables
      client-side computed columns — currently unreachable in 0.4.
- [ ] **`with_expression(name, closure)` where the closure receives the table
      reference** — lets a computed field look up sibling columns or traverse
      references. Legacy: `vantage/src/sql/table/with_queries.rs`.

# References & subqueries (missing shapes)

0.4 has `with_one`, `with_many`, `with_foreign`, `get_ref`, `get_ref_as`,
`get_subquery_as`. Missing:

- [ ] **Untyped `get_subquery()`** — correlated-subquery form that returns
      `AnyTable` / an expression, for cases where the caller doesn't know the
      target entity type at compile time. 0.4 has `get_subquery_as<E2>()`
      (typed) at `vantage-table/src/table/impls/refereces.rs:164` but no
      untyped parallel to `get_ref()`.
- [ ] **`get_ref_related()`** — a variant used when assembling expressions
      rather than datasets (legacy had both). Currently conflating
      "navigate to related dataset" and "embed related rows in an expression"
      via the same API.
- [ ] **Multi-hop reference traversal** — legacy supported chains like
      `Product::table().get_ref("orders").get_ref("client")` producing nested
      `IN (SELECT ... FROM ... WHERE ... IN (SELECT ...))`. Documented in
      `docs2/src/5.1-reference-traversal.md`. 0.4's `get_ref()` returns
      `AnyTable`, which makes chaining awkward — confirm it works end-to-end
      and, if not, fix the ergonomics.

# Associated query / cross-datasource execution

- [ ] **`AssociatedQuery<T, E>`** — a full query bound to a datasource with
      `.fetch()`, `.with_skip()`, `.with_limit()`, etc. Legacy:
      `vantage/src/datasource/associated_query.rs`. 0.4's `AssociatedExpression`
      is narrower (single value / scalar aggregate); an associated *query* with
      skip/limit/order is what enables materialising cross-datasource result
      sets.
- [ ] **`RouterDataSet`** — route reads to one backend and writes to another
      (cache-ahead, read-replica, write-through patterns). `vantage-live`'s
      `LiveTable` overlaps but is specifically the cache scenario;
      `RouterDataSet` was more general.

# Table aliasing infrastructure

Only column-level `with_alias()` exists in 0.4
(`vantage-table/src/column/core.rs:59`). Table-level aliasing and the alias
collision logic are gone.

- [ ] **`TableAlias` struct + table-level `with_alias()`** — required for any
      case where the same table appears twice in a query (self-joins,
      multi-relationship joins). Legacy: supporting types in
      `vantage/src/sql/table/alias.rs`.
- [ ] **`UniqueIdVendor`** — generates non-clashing short aliases (`u`, `us`,
      `use`, `user`, ...) when building composite queries. Legacy:
      `vantage/src/uniqid.rs`. Needed before the TODO.md "resolve clashes in
      table aliases" item can be cleanly implemented.
- [ ] **`enforce_table_in_field_queries()`** — force table prefix on column
      references when context is ambiguous (e.g. after a join introduced a
      same-named column). Legacy: `vantage/src/sql/table/alias.rs`.

# Conditions / operations

0.4 has `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `in_`, `in_list` on columns
(`vantage-table/src/operation.rs`). Missing:

- [ ] **`is()` / `is_not()` / NULL checks** — no `IS NULL` / `IS NOT NULL` on
      the `Operations` trait. Legacy had `column.is(Value::Null)`. Grep for
      `is_null`, `is_not_null`, `IS NULL` in `vantage-*/src/` returns nothing.
- [ ] **Nested condition composition** — legacy `Condition::from_condition()`
      let you build arbitrary AND/OR trees. Confirm whether 0.4 supports
      arbitrary nesting or only the flat AND-of-conditions attached to a table;
      if the latter, list it. (TODO.md's `Condition::or()` n-ary limit is a
      related but narrower entry.)

# Datasource coverage gaps

- [ ] **In-process SQL-dialect-faithful mock** — legacy
      `vantage/src/mocks/rusqlite.rs` provided a rusqlite-backed in-process
      mock for unit tests that actually exercised the SQL dialect. 0.4 has
      `MockTableSource` + `ImTable` (JSON in-memory), which don't catch
      SQL-specific regressions. Tests currently depend on a real SQLite/Postgres/MySQL
      database via `ingress.sh`.

# Entity conveniences

`ActiveEntity` in 0.4 (`vantage-dataset/src/record.rs`) has `.save()` but not:

- [ ] **`reload()`** — refetch the entity from the datasource in place, for use
      after external mutations.
- [ ] **`delete()`** — delete the entity by id without reaching back to the
      table. Currently callers have to remember the id and call
      `table.delete(id)` themselves.

Legacy: `AssociatedEntity<T>` had both, plus `.id()`. (0.4's `ActiveEntity`
stores the id internally; it's just not exposed as a convenience method.)

# Documentation gaps

Several features that *do* exist (or will exist once the above are ported) had
dedicated docs that aren't present in docs4 yet:

- [ ] **Operation hooks** — docs2 §4.7 (`docs2/src/4.7-operation-hooks.md`).
- [ ] **Unions** — docs2 §5.3.
- [ ] **Cross-source operations** — docs2 §5.5.
- [ ] **Caching / domain-specific extensions / API integration / UI generation**
      — docs2 §6.1 / §6.2 / §6.3 / §6.4. Some of this is implicit in
      `vantage-live` / `vantage-api-client` / `vantage-ui-adapters` but
      there's no user-facing narrative in docs4.
- [ ] **Performance optimisation** — docs2 §7.
- [ ] **Multi-tenancy patterns** — tenant isolation via conditional field
      restrictions, documented in docs2 §6.2 and exercised in the bakery
      example. No equivalent guidance in docs4.

# Tracked elsewhere

Known gaps already in `TODO.md` — not re-listed above:

- Transactions (TODO.md → Architecture)
- `Condition::or()` n-ary (TODO.md → Architecture)
- Table-level GROUP BY / aggregations (TODO.md → Someday maybe)
- Disjoint subtypes pattern (TODO.md → Someday maybe)
- Cross-datasource operations (TODO.md → Someday maybe)
- Realworld example application (TODO.md → Someday maybe)
- Idempotent CRUD / replayability (TODO.md → Someday maybe)
- `Table::join_table` condition preservation + alias clashes (TODO.md → Architecture)
- `returning id` picks the correct id column; `with_id()` shouldn't need `.into()` (TODO.md → Architecture)
- UUID + `Vec<u8>` type-system coverage (TODO.md → Type System)
- CI: automate crate publishing, rebuild book on `Cargo.toml` changes (TODO.md → CI/CD)
- `sql_fx!()` macro, `Expression::empty()` sweep, PostgreSQL ingress split (TODO.md → Query Builder)
- MongoDB ObjectId-only IDs, `get_count`/`get_sum`/`get_max`/`get_min` trait-boundary fix,
  `ReadableDataSet::get` returning `Result<Option<E>>`, `column_table_values_expr`
  decoupling, `Selectable` parameterised on condition type (TODO.md → Trait boundary fixes)
