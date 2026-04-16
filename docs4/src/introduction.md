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

- [ ] Expression basics (`sqlite_expr!`, parameter binding, injection safety)
- [ ] Identifier quoting (`ident()`, dialect-aware)
- [ ] SqliteSelect via Selectable trait (with_source, with_field, with_condition, with_order,
      with_limit)
- [ ] Executing queries (ExprDataSource, `db.execute()`, `db.associate()`)
- [ ] Aggregates on select (as_count, as_sum)
- [ ] Table definition (Table::new, with_column_of, with_id_column, builder pattern)
- [ ] Table → select query (table.select() returns vendor-specific builder)
- [ ] DataSource concept (what it is, how you pass it to Table::new)
- [ ] Entity struct (plain Rust struct, no id field, `#[entity]` macro)
- [ ] Record\<V\> (persistence-native value bags)
- [ ] Type system (vantage_type_system! macro, AnySqliteType, typed vs untyped values)
- [ ] ReadableDataSet (list, get, get_some, get_count)
- [ ] WritableDataSet (insert, delete)
- [ ] ActiveEntity (get_entity, modify via DerefMut, save)
- [ ] Conditions (table["field"], .eq/.gt/.lt, with_condition returns new table)
- [ ] Sync vs async — defining table is sync, hitting DB is async
- [ ] Relationships (with_one, with_many, declared on table not entity)
- [ ] Reference traversal (get_ref_as, subquery-based conditions)
- [ ] Computed fields (with_expression, correlated subqueries)
- [ ] AnyTable (from_table, JSON boundary, type-erased generic code)
- [ ] Model crate pattern (entities + table constructors + relationships in one crate)
- [ ] Multiple backends per entity (surreal_table, sqlite_table, csv_table)
- [ ] DataSet trait hierarchy (ReadableDataSet, InsertableDataSet, WritableDataSet)
- [ ] ValueSet traits (schema-less Record-based access)
- [ ] Vendor-specific query builders (SqliteSelect, PostgresSelect, SurrealSelect)
- [ ] Deferred expressions (cross-database defer/map)
- [ ] Progressive persistence model (implement only what your backend supports)
- [ ] Error handling (VantageError, context, with_context)
- [ ] Connection management (DSN pattern, OnceLock)
- [ ] Pagination (with_pagination)
- [ ] Search expressions (search_expression, vendor-specific LIKE/CONTAINS)
- [ ] models! macro (all_tables() generation)
