# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2025-02-16

- Refactored Column aliases with RwLock, implementing `join` properly #49
- Migrated from tokio_postgres to sqlx #47
- table::with_column now accepts Column struct #48

In this version I have done a lot of soul-searching trying to understand what `vantage` is
and how I should continue to evolve it in the future. Here are some thought:

- I have created a draft "extension" for vantage (https://github.com/romaninsh/vantage_scheduling)
  which illustrates potential way to add "scheduling" functionality to fully user-defined
  table types. In the future this could be a way to create specialised non-generic extensions.
- I have considered how to support custom types in columns. Currently columns can be strings,
  numbers or bools, but we can't use UUID or Chrono date types. To have this support, Expression
  would need to be rewritten using `dyn` nesting.
- I have considered how no-SQL sync query building could work. I will add some tests later, but
  the goal is to dynamically support operations over no-SQL persistences and stack operations
  until the read (or other operations) are performed.
- I have started work on converting Column into `dyn` to allow further extensions, specifically
  to support custom types in columns.
- Finally I have had a lot of brainstorming on how `vantage` could drive dynamic UI components.
  To do that I will need to implement more traits for Reflection-like functionality, that would
  drive dynamic UI or generic API endpoints.

I will continue to slowly work on new features and in a meantime - please reach out to me if you
have any questions or suggestions.

## [0.1.0] - 2024-12-12

### 🚀 Features

- Query Building - added `Query` with `set_type`, `fields`, and `build`
- Introduced `Renderable` trait (renamed into `Chunk`)
- Table buildind - added `Table`
- Introcuded `Field` struct
- Introduced `Select`, `Insert`, `Update`, `Delete` query types
- Introduced `Expression` struct
- Implemented Field positional rendering
- Introduced `ReadableDataSet` and `WritableDataSet` traits
- Added `mocking` for unit tests
- Briefly introduced and removed `sqlite` support
- Implemented `Postgres` datasource
- Introduced Conditions
- Implemented nested expressions
- Implemented Operations (such as field.eq(5))
- Implemented DataSource generics with <D: DataSource>
- Added `Table.sum()`
- Added `AssociatedQuery` and `AssociatedExpressionArc`
- Added `Query.join()`
- Added `with_one` and `with_many` into `Table` for relation definitions
- Added lazy expressions with `with_expression`
- Implementet Entity generics with <E: Entity>
- Added `Entity` trait and `SqlTable` trait

### 📚 Documentation

- Added mdbook documentation under `docs`
- Added rustdoc documentation under `vantage`

### ⚙️ Miscellaneous Tasks

- Added bakery example under `bakery_example`
- Added API example under `bakery_api`
