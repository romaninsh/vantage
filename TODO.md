# MongoDB PoC & Trait Boundary Improvements (from MongoDB work, 2026-04)

## Completed

- [x] Added `type Condition` associated type to `TableSource` — allows non-Expression condition
      types (e.g. `bson::Document` for MongoDB). SQL/SurrealDB backends use
      `type Condition = Expression<Self::Value>` (no change).
- [x] MongoDB type system (`AnyMongoType`, `bson::Bson` value type, bson v2)
- [x] MongoDB `TableSource` impl — full CRUD using mongodb driver directly, no Expression queries.
      `type Condition = MongoCondition` for native MongoDB filters with deferred support.
- [x] **Refactored reference system** — replaced `RelatedTable` trait with `Reference` trait.
      `HasOne`/`HasMany` describe relationships (field names + factory). Resolution happens in
      `Table::get_ref_as` via `related_in_condition`. Factory takes `T` (data source) instead of
      closures — enables `with_many("orders", "client_id", Order::postgres_table)`.
- [x] **Added `HasForeign` for cross-persistence refs** — `with_foreign()` accepts a closure that
      returns `AnyTable` with deferred conditions. Enables lazy cross-backend traversal.
- [x] **`get_ref_as` takes one type param** — `get_ref_as::<Order>("orders")` instead of
      `get_ref_as::<PostgresDB, Order>("orders")`. Backend type inferred from self.
- [x] **`get_ref()` returns `AnyTable`** — works for both same-backend and foreign refs via
      `Reference::resolve_as_any`.
- [x] MongoDB relationship traversal working — `with_one`/`with_many` + `get_ref_as` tested for
      has_many and has_one patterns.
- [x] MongoDB search regex escaping — metacharacters escaped, empty columns return always-false.
- [x] MongoDB `related_in_condition` uses projected query (only fetches needed column).
- [x] **`with_expression` / `get_subquery_as` / `related_correlated_condition`** — Table supports
      computed expression fields via closure-based lazy evaluation. `get_subquery_as` produces
      correlated conditions (`target.fk = source.id`) for embedding subqueries in SELECT.
      `related_correlated_condition` on `TableSource` (default `unimplemented!`). Implemented for
      Postgres, MySQL, SQLite (`ident().dot_of()`), SurrealDB (`$parent` syntax). Tested via CLI
      across Postgres, SQLite, SurrealDB with `order_count` expression on Client table.

## Trait boundary fixes needed

- [ ] **Move `get_count`/`get_sum`/`get_max`/`get_min` off `SelectableDataSource`** — currently in
      `table/impls/selectable.rs` behind `T: SelectableDataSource`. They just delegate to
      `TableSource` methods. Move to a separate impl block requiring only `T: TableSource` so
      MongoDB and other non-query backends can use them directly.
- [ ] **Remove `delete`/`delete_all` from `WritableDataSet`** — `WritableValueSet` is the canonical
      place for deletion (doesn't require entity type). Having both causes ambiguity when calling
      `table.delete()`. Keep only in `WritableValueSet`.
- [ ] **Decouple `column_table_values_expr` from `ExprDataSource`** — the method returns
      `AssociatedExpression` which forces `ExprDataSource` dependency. Consider moving to a
      sub-trait so non-SQL backends don't carry dead code. SQL backends use it internally in
      `related_in_condition`; MongoDB never touches it.
- [ ] **Explore `Selectable` parameterized on condition type** — currently `add_where_condition`
      takes `impl Expressive<T>`, hardcoding Expression-based conditions. MongoDB could implement
      its own `select()` if `Selectable` (or a parallel trait) accepted `Condition` type directly.
- [x] ~~**Analyse `with_one`/`with_many` for non-Expression backends**~~ — **RESOLVED**: Refactored
      reference system. `HasOne`/`HasMany` use `related_in_condition` (each backend builds its own
      native condition). `From<Expression<T::Value>>` bound still exists on `with_one`/`with_many`
      via `resolve_as_any` but MongoDB implements it as a no-op panic (never called for traversal —
      `get_ref_as` uses `related_in_condition` directly). Could be cleaned up further by moving
      `resolve_as_any` bounds into the impl block.

## Cleanup (lower priority)

- [ ] **Remove `From<Expression<AnyMongoType>> for MongoCondition` panic impl** — exists only to
      satisfy trait bounds. Could be eliminated by separating the `resolve_as_any` bounds or
      splitting `with_one`/`with_many` bounds from the `Reference` impl bounds.
- [ ] **Consider removing `related_in_condition` from `TableSource`** — now only used by
      `Table::get_ref_as` (same-backend resolution). Could be moved into the `HasOne`/`HasMany`
      `resolve_as_any` implementations directly, removing it from the trait surface.

# Query Builder Improvements (from MySQL work, 2026-04)

- [ ] `expr.as_alias()` — add alias method on `Expression<T>`, then remove `Option<String>` from
      `with_expression` and all `with_alias()` from primitives (Fx, Iif, Concat, GroupConcat,
      JsonExtract, DateFormat, Case). Also fixes Fx alias hardcoding `"` instead of backticks.
- [ ] `sql_fx!()` macro — mixed-type args for function calls:
      `sql_fx!("find_in_set", "write", (ident("permissions")))` instead of wrapping every arg in
      `mysql_expr!`
- [ ] PostgreSQL ingress — split into `vantage_v2`, `vantage_v3`, `vantage_v4_pg` with DROP+CREATE,
      matching MySQL pattern
- [ ] `Expression::empty()` sweep — replace all `Expression::new("", vec![])` across the codebase

# v0.2 (eta January 2025)

- [x] Swap to sqlx

# v0.3 PRERELEASE

- [ ] Implement `only_column()` method for SurrealSelect query builder
- [x] Implement prelude for vantage_surrealdb to avoid manual imports
- [x] Fix get() method to accept (&select) instead of requiring select.expr() - should work with
      IntoExpression trait
- [ ] **BUG**: SurrealDB IN subquery returns record objects not scalar values
  - Reference traversal generates `WHERE bakery IN (SELECT id FROM bakery WHERE ...)`
  - SurrealDB returns `{id: "bakery:hill_valley"}` from subquery, not `"bakery:hill_valley"`
  - Need `SELECT VALUE id` but that's SurrealDB-specific, not in generic Selectable trait
  - Workaround: Add `select_value()` to Selectable trait or handle in SurrealDB adapter
  - Affects: Reference traversal in bakery_model4 (e.g., `bakery ref products list`)

# v0.3 (Eta 2025)

- [-] Refactor Expressions and separate it into a module
  - [ ] Split out "Owned" and "Lazy" expressions
  - [ ] Implement vendor-specific expressions
  - [ ] Initial implementation of SurrealDB SQL syntax
  - [ ] Use dyn/into patterns for cleaner syntax
- [-] Allow use of custom `dyn` columns
- [ ] Add a sample CSV table implementation
- [ ] "returning `id` should properly choose ID column"
- [ ] Add thread safety (currently tests in bakery_api fail)
- [ ] Implement transaction support
- [ ] Add MySQL support
- [ ] Add a proper database integration test-suite
- [ ] Implement all basic SQL types
- [ ] Implement more operations
- [ ] Fully implement joins
- [ ] Implement and Document Disjoint Subtypes pattern
- [ ] Add and document more hooks
- [ ] Comprehensive documentation for mock data testing
- [ ] Implement "Realworld" example application in a separate repository
- [ ] Implement Uuid support
- [ ] with_id() shouldn't need into()

# v0.3

- [ ] Implement associated records (update and save back)
- [ ] Implement table aggregations (group by)
- [ ] Implement NoSQL support
- [ ] Implement RestAPI support
- [ ] Implement Queue support
- [ ] Add expression as a field value (e.g. when inserting)
- [ ] Add delayed method evaluation as a field value (e.g. when inserting)
- [ ] Add tests for cross-database queries
- [ ] Explore replayability for idempotent operations and workflow retries
- [ ] Provide example for scalable worker pattern

# Someday maybe:

- [ ] Implement todo in update() in WritableDataSet for Table
- [ ] Continue through the docs - align crates with documentation

# Create integration test-suite for SQL testing

- [ ] Create separate test-suite, connect DB etc
- [ ] Make use of Postgres snapshots in the tests
- [ ] Add integration tests for update() and delete() for Table

# Control field queries

- [ ] add tests for all CRUD operations (ID-less table)
- [ ] implemented `each` functionality for DataSet
- [ ] implement functions: (concat(field, " ", otherfield))
- [ ] move postgres integration tests into a separate test-suite
- [ ] add tests for table conditions (add_condition(field1.eq(field2))
- [ ] implement sub-library for datasource, supporting serde
- [ ] add second data-source (csv) as an example
- [ ] add sql table as a dataset at a query level (+ clean up method naming)
- [ ] postgres expressions should add type annotation into query ("$1::text")

Implement extensions:

- [ ] Lazy table joins (read-only)
- [x] Implement add_field_lazy() — implemented as `with_expression` on Table

Minor Cases:

- [ ] Table::join_table should preserve conditions on other_table
- [ ] Table::join_table should resolve clashes in table aliases
- [ ] Condition::or() shouldn't be limited to only two arguments
- [ ] It should not be possible to change table alias, after ownership of Fields is given

## Implement cross-datasource operations

Developers who operate with the models do not have to be aware of the data source. If you want to
implement this, then you can define your data sets to rely on factories for the data-set:

```rust
let client_set = ClientSet::factory();
let client = client_set.load_by_auth(auth_token)?;
let basket = client.basket();  // basket is stored in session/memory
for item in basket.items()?.into_iter() {
    let stock = item.stock();
    stock.amount -= item.amount;
    stock.save();  // saves into SQL

    item.status = "sold";
    item.save();   // item data is stored in cache
}
basket.archive();  // stores archived basked into BigQuery
```

## Implement in-memory cache concept

This allows to create in-memory cache of a dataset. Finding a record in a cache is faster. Cache
will automatically invalidate items if they happen to change in the database, if the datasource
allows subscription for the changes. There can also be other invalidation mechanics.

Cache provides a transparent layer, so that the business logic code would not be affected.

```rust
let client_set = ClientSet::new(ClientCache::new(postgres));
// use client_set as usual
```
