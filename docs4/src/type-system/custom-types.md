# Adding Custom Types

You can teach Vantage to store any Rust type by implementing the persistence type trait. Here's the
pattern, using an `Animal` enum that maps to a text column across every backend:

1. **Define your type** — a plain Rust enum (or struct).
2. **Implement the persistence trait** — `SqliteType`, `PostgresType`, etc. Each impl says how to
   convert to/from the storage format (CBOR for SQL backends, BSON for MongoDB).
3. **Use it in expressions** — once the trait is implemented, `sqlite_expr!("species = {}", animal)`
   just works.

A single type can implement traits for multiple backends, so the same `Animal` enum works with
SQLite, Postgres, SurrealDB, MongoDB, and CSV — each with its own serialization logic.

<!-- TODO: full walkthrough with code, inspired by bakery_model3/src/animal.rs -->
