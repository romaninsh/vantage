# Adding a New Persistence

So you want to connect Vantage to a new database? This guide walks through the process in eight
incremental steps — each one unlocks more framework features. You don't have to implement all
eight; stop whenever your persistence has enough capability for your use case.

<!-- toc -->

---

## Overview

| Step                                                          | What you build                                                | What it unlocks                                                      | Can skip?                                      |
| ------------------------------------------------------------- | ------------------------------------------------------------- | -------------------------------------------------------------------- | ---------------------------------------------- |
| [1. Type System](./new-persistence/step1-types.md)            | `vantage_type_system!` macro, `AnyType`, `Record` conversions | Type-safe values, struct ↔ record mapping                            | **Required**                                   |
| [2. Expressions](./new-persistence/step2-expressions.md)      | Vendor macro, `ExprDataSource`                                | Execute raw queries, cross-database `defer()`                        | Skip for simple read-only sources (CSV)        |
| [3. Operators](./new-persistence/step3-operators.md)          | Vendor operation trait, typed `.eq()`/`.gt()`/`.in_()`        | Ergonomic typed conditions instead of raw expression macros          | Skip if you don't expose typed columns         |
| [4. Query Builder](./new-persistence/step4-query-builder.md)  | `Selectable`, `SelectableDataSource`                          | Composable SELECT with conditions, ordering, limits                  | Skip if your persistence has no query language |
| [5. Table & CRUD](./new-persistence/step5-table-crud.md)      | `TableSource`, entity tables, aggregates, writes              | `Table<DB, Entity>`, full CRUD, `ReadableDataSet`, `WritableDataSet` | **Required** for table support                 |
| [6. Relationships](./new-persistence/step6-relationships.md)  | `with_one`, `with_many`, correlated subqueries                | Reference traversal, expression fields                               | Skip if you don't need cross-table queries     |
| [7. Multi-Backend](./new-persistence/step7-multi-backend.md)  | `AnyTable::from_table()`, CLI example                         | Type-erased tables, generic UI/API code                              | Skip if you only use one persistence           |
| [8. Vista](./new-persistence/step8-vista-integration.md)      | `<Driver>VistaFactory`, `<Driver>TableShell`, YAML extras     | YAML-defined data handles consumed by UI / scripting / agents        | Skip if you don't need Vista support           |

---

## Step 1: Type System

Every database has its own idea of what types exist. The `vantage_type_system!` macro generates a
type trait, variant enum, and type-erased `AnyType` wrapper that prevents silent casting between
incompatible types.

You'll implement the type trait for each Rust type your database supports, set up `Record`
conversions (free via serde for JSON-based backends, or via `#[entity]` macro for custom value
types), and add `TryFrom<AnyType>` for scalar extraction.

**[Read Step 1 →](./new-persistence/step1-types.md)**

---

## Step 2: Expressions

With types in place, build a vendor macro (`sqlite_expr!`, `surreal_expr!`) that produces
`Expression<AnyType>` with typed parameters. Implement `ExprDataSource` to execute expressions
against your database — handling parameter binding, deferred cross-database resolution, and result
parsing.

Skip this step if your persistence evaluates conditions in-memory (like CSV) — you can implement
`TableSource` directly without an expression engine.

**[Read Step 2 →](./new-persistence/step2-expressions.md)**

---

## Step 3: Operators

Build vendor-specific operation traits so typed columns get ergonomic `.eq()`/`.gt()`/`.in_()`
methods that produce your backend's native condition type. Blanket-implemented over all
`Expressive<T>` so users don't have to write raw expression macros for every condition.

Skip this step if you don't expose typed columns to end users.

**[Read Step 3 →](./new-persistence/step3-operators.md)**

---

## Step 4: Query Builder

Build a SELECT struct implementing the `Selectable` trait — fields, conditions, ordering, limits,
aggregates. Wire it up through `SelectableDataSource` so the rest of Vantage can create and execute
queries through a standard interface.

Skip this step if your persistence doesn't have a query language. MongoDB, for instance, skips
`Selectable` and uses native BSON pipelines instead.

**[Read Step 4 →](./new-persistence/step4-query-builder.md)**

---

## Step 5: Table & CRUD

Implement `TableSource` to give Vantage full table abstraction — columns, conditions, ordering,
pagination, entity CRUD, and aggregates. This is where `Table<DB, Entity>` comes alive and
auto-implements `ReadableDataSet`, `WritableDataSet`, and `ActiveEntitySet`.

Start with `todo!()` for every method and implement them incrementally, driven by tests.

**[Read Step 5 →](./new-persistence/step5-table-crud.md)**

---

## Step 6: Relationships

Declare `with_one` and `with_many` relationships on tables and traverse them with `get_ref_as`.
Implement `column_table_values_expr` for subquery-based traversal and optionally
`related_correlated_condition` for correlated subqueries (expression fields like computed counts).

Skip this step if your persistence is flat (no foreign keys or cross-collection references).

**[Read Step 6 →](./new-persistence/step6-relationships.md)**

---

## Step 7: Multi-Backend Applications

Wrap your tables with `AnyTable::from_table()` to erase the backend type. This enables generic UI,
CLI, and API code that works identically across SurrealDB, SQLite, CSV, MongoDB, or your new
persistence — all through a uniform `serde_json::Value`-based interface.

**[Read Step 7 →](./new-persistence/step7-multi-backend.md)**

---

## Step 8: Vista

Wrap your typed `Table<Driver, E>` as a `Vista` — the universal, schema-bearing handle that UI,
scripting, and agents consume through a CBOR / `String` boundary. Adds YAML schema loading and
condition delegation so callers don't need a Rust entity struct.

Skip this step if your driver is only consumed from typed Rust code.

**[Read Step 8 →](./new-persistence/step8-vista.md)**
