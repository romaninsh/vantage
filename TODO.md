# MongoDB PoC & Trait Boundary Improvements (from MongoDB work, 2026-04)

## MongoDB reference traversal

- [x] **Make `with_one` / `with_many` usable across `ObjectId` / `String` id-field boundaries**
      — `related_in_condition` now pushes both the raw value and its alternate representation
      (ObjectId's hex string, or the parsed ObjectId of a hex-string value) into the `$in`
      filter, so traversal works whether the target's FK is stored as `ObjectId` or as a
      plain string. Also added `impl From<MongoId> for AnyMongoType` so user-land narrowing
      by id (`c.id().eq(MongoId::parse(...))`) dispatches to the right BSON type.
- [ ] **Drop the `String` variant from `MongoId`** — commit the framework to `ObjectId`-only
      ids and lean on Mongo's native convention. Simplifies `id.rs`, removes the dual-push
      hack in `related_in_condition`, and drops the smart-parse paths added to
      `AnyMongoType`/`MongoId`. Blast radius: `scripts/db/v2.js` seeds string `_id`s
      (`"hill_valley"`, `"order1"`, etc.) and `tests/5_references.rs` asserts them directly —
      both need rewriting to real ObjectIds. Users who genuinely want string-keyed documents
      can model them in a non-`_id` field. ~0.5–1 day including test fixture rewrite.

## Trait boundary fixes needed

- [ ] **Move `get_count`/`get_sum`/`get_max`/`get_min` off `SelectableDataSource`** — currently in
      `table/impls/selectable.rs` behind `T: SelectableDataSource`. They just delegate to
      `TableSource` methods. Move to a separate impl block requiring only `T: TableSource` so
      MongoDB and other non-query backends can use them directly.
- [x] **Remove `delete`/`delete_all` from `WritableDataSet`** — `WritableValueSet` is the canonical
      place for deletion (doesn't require entity type). Having both causes ambiguity when calling
      `table.delete()`. Keep only in `WritableValueSet`.
- [ ] **Change `ReadableDataSet::get(id)` to return `Result<Option<E>>`** — current contract
      returns `Err` when the row is missing, which forces consumers (e.g. the axum tutorial's
      `From<VantageError> for ApiError` impl) to string-match `"no row found"` to produce 404s.
      Options: (a) additive — add `get_opt(id) -> Result<Option<E>>`, leave `get` as-is; (b) full
      contract change — `get` itself returns `Result<Option<E>>`. Preference: (a) first, migrate
      callers over time; `Option` is the Rust-native way to express "lookup missed".
- [ ] **Decouple `column_table_values_expr` from `ExprDataSource`** — the method returns
      `AssociatedExpression` which forces `ExprDataSource` dependency. Consider moving to a
      sub-trait so non-SQL backends don't carry dead code. SQL backends use it internally in
      `related_in_condition`; MongoDB never touches it.
- [ ] **Explore `Selectable` parameterized on condition type** — currently `add_where_condition`
      takes `impl Expressive<T>`, hardcoding Expression-based conditions. MongoDB could implement
      its own `select()` if `Selectable` (or a parallel trait) accepted `Condition` type directly.

## Cleanup (lower priority)

- [ ] **Remove `From<Expression<AnyMongoType>> for MongoCondition` panic impl** — exists only to
      satisfy trait bounds. Could be eliminated by separating the `resolve_as_any` bounds or
      splitting `with_one`/`with_many` bounds from the `Reference` impl bounds.
- [ ] **Consider removing `related_in_condition` from `TableSource`** — now only used by
      `Table::get_ref_as` (same-backend resolution). Could be moved into the `HasOne`/`HasMany`
      `resolve_as_any` implementations directly, removing it from the trait surface.

# Type System — missing entity-level impls

- [ ] `Vec<u8>` — binary data (BLOB/BYTEA/BLOB), bind/read paths already exist, needs `impl XxxType`
- [ ] `Uuid` — Postgres has native UUID column + variant, MySQL uses CHAR(36); `uuid` crate

# Query Builder Improvements (from MySQL work, 2026-04)

- [x] `expr.as_alias()` — `AliasExt` blanket impl in vantage-sql, removed `Option<String>` from
      `Selectable::with_expression`, stripped alias from all primitives (Fx, Iif, Concat,
      GroupConcat, JsonExtract, DateFormat, Case, Ternary). Fixes Fx/Case hardcoded `"` quoting.
- [ ] `sql_fx!()` macro — mixed-type args for function calls:
      `sql_fx!("find_in_set", "write", (ident("permissions")))` instead of wrapping every arg in
      `mysql_expr!`
- [ ] PostgreSQL ingress — split into `vantage_v2`, `vantage_v3`, `vantage_v4_pg` with DROP+CREATE,
      matching MySQL pattern
- [ ] `Expression::empty()` sweep — replace all `Expression::new("", vec![])` across the codebase

# SurrealDB

- [ ] Implement `only_column()` method for SurrealSelect query builder
- [ ] **BUG**: SurrealDB IN subquery returns record objects not scalar values
  - Reference traversal generates `WHERE bakery IN (SELECT id FROM bakery WHERE ...)`
  - SurrealDB returns `{id: "bakery:hill_valley"}` from subquery, not `"bakery:hill_valley"`
  - Need `SELECT VALUE id` but that's SurrealDB-specific, not in generic Selectable trait
  - Affects: Reference traversal in bakery_model4 (e.g., `bakery ref products list`)

# CI/CD

- [ ] **Automate crate publishing in CI** — add a workflow that publishes crates to crates.io
      on tag/release, in dependency order. Require version bump (reject if version matches
      what's already on crates.io).
- [ ] **Rebuild book on Cargo.toml changes** — the book workflow currently only triggers on
      `docs4/**` changes. Version bumps update rustdoc links, so the book should also rebuild
      when any `Cargo.toml` in the workspace changes.

# Architecture

- [ ] Refactor Expressions — split out "Owned" and "Lazy" expressions, use dyn/into patterns
- [ ] Implement transaction support
- [ ] `returning id` should properly choose ID column
- [ ] `with_id()` shouldn't need `into()`
- [ ] Add a sample CSV table implementation
- [ ] Table::join_table should preserve conditions on other_table
- [ ] Table::join_table should resolve clashes in table aliases
- [ ] Condition::or() shouldn't be limited to only two arguments

# Someday maybe

- [ ] Implement associated records (update and save back)
- [ ] Implement table aggregations (group by)
- [ ] Implement RestAPI support
- [ ] Implement Queue support
- [ ] Add expression as a field value (e.g. when inserting)
- [ ] All persistences should implement idempotent CRUD — `insert()` with duplicate ID should
      succeed silently (INSERT OR IGNORE / ON CONFLICT DO NOTHING). Currently only `replace()`
      and `delete()` are idempotent.
- [ ] Explore replayability for idempotent operations and workflow retries
- [ ] Implement and Document Disjoint Subtypes pattern
- [ ] Implement "Realworld" example application in a separate repository
- [ ] In-memory cache layer with transparent invalidation
- [ ] Cross-datasource operations (business logic agnostic to storage backend)
