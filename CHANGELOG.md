# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2025-02-16

- Adopted sqlx
- Various improvements

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
