# Persistence-aligned Type System

Every Vantage persistence defines its own type trait — `SqliteType`, `PostgresType`, `SurrealType`,
`MongoType`, `CsvType` — that maps Rust values to and from the storage format. This is how
expressions like `sqlite_expr!("price > {}", 150i64)` know how to bind `150i64` as a parameter.

Built-in implementations cover the common ground: `bool`, `i64`, `f64`, `String`,
`Option<T>`, `chrono` date/time types, and more. Each persistence supports exactly the types its
backend can handle natively.

Because the trait is open, you can implement it for your own types — enums, newtypes, domain
objects — and they'll work everywhere expressions are used: conditions, inserts, updates.

See [Adding Custom Types](./type-system/custom-types.md) for a walkthrough.
