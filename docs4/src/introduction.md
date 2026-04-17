# Vantage Framework

Vantage is a data entity persistence and abstraction framework for Rust.

Rather than being a traditional ORM, Vantage introduces the concept of a **DataSet** — an abstract,
composable handle to records living in a remote data store. You define structure, conditions,
relations, and operations without loading data eagerly, and Vantage translates your intent into
efficient queries for whichever backend you're using.

This documentation covers Vantage **0.4**.

## Getting Started

Vantage covers a lot of ground — multiple databases, type systems, entity frameworks, UI adapters —
but none of that matters until you've seen it do something useful.

This guide introduces Vantage concepts one at a time, each building on the last. We'll start with
something you already know — SQL — and work our way up to the bigger abstractions. Along the way
we'll build a small CLI tool that grows with each chapter.

You'll need basic Rust experience (structs, traits, async/await, cargo). No prior Vantage knowledge
required.

**Start here:** [SQLite and the Query Builder](./intro/step1-first-query.md)

---

<!-- internal use: concept coverage tracker — once covered in introduction, cross out here -->

## TODO: Concept Coverage

- [x] Expression basics (`sqlite_expr!`, parameter binding, injection safety)
- [x] Identifier quoting (`ident()`, dialect-aware)
- [ ] SqliteSelect via Selectable trait (with_source, with_field, with_condition, with_order,
      with_limit) — partial, missing with_order, with_limit
- [x] Executing queries (ExprDataSource, `db.execute()`) — `db.associate()` not covered
- [ ] Aggregates on select (as_count, as_sum) — covered via `db.aggregate()` not `.as_count()` directly
- [x] Table definition (Table::new, with_column_of, with_id_column, builder pattern)
- [x] Table → select query (table.select() returns vendor-specific builder)
- [x] DataSource concept (what it is, how you pass it to Table::new)
- [x] Entity struct (plain Rust struct, no id field, `#[entity]` macro)
- [x] Record\<V\> (persistence-native value bags)
- [ ] Type system (vantage_type_system! macro, AnySqliteType, typed vs untyped values) — AnySqliteType mentioned, macro not shown
- [x] ReadableDataSet (list, get, get_some, get_count)
- [x] WritableDataSet (insert, delete)
- [ ] ActiveEntity (get_entity, modify via DerefMut, save)
- [x] Conditions (table["field"], .eq/.gt/.lt, with_condition returns new table)
- [ ] Sync vs async — defining table is sync, hitting DB is async
- [x] Relationships (with_one, with_many, declared on table not entity)
- [x] Reference traversal (get_ref_as, subquery-based conditions)
- [x] Computed fields (with_expression, correlated subqueries)
- [x] AnyTable (from_table, JSON boundary, type-erased generic code)
- [ ] Model crate pattern (entities + table constructors + relationships in one crate)
- [ ] Multiple backends per entity (surreal_table, sqlite_table, csv_table)
- [ ] DataSet trait hierarchy (ReadableDataSet, InsertableDataSet, WritableDataSet) — partial, ReadableDataSet only
- [ ] ValueSet traits (schema-less Record-based access)
- [x] Vendor-specific query builders (SqliteSelect, PostgresSelect, SurrealSelect)
- [ ] Deferred expressions (cross-database defer/map)
- [ ] Progressive persistence model (implement only what your backend supports)
- [x] Error handling (VantageError, context, with_context) — partial (VantageError + context shown)
- [ ] Connection management (DSN pattern, OnceLock)
- [ ] Pagination (with_pagination)
- [x] Search expressions (search_expression, vendor-specific LIKE/CONTAINS) — via `with_search`
- [ ] models! macro (all_tables() generation)
- [x] Extension traits (trait CategoryTable, ref_products, adding custom methods like print)
