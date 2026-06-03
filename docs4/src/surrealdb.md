# SurrealDB

The `vantage-surrealdb` crate targets [SurrealDB](https://surrealdb.com) — a document-graph database
whose query language (SurrealQL) is close enough to SQL to share a vocabulary, but diverges where it
matters: graph traversals instead of joins, embedded arrays with closures, record links, and a
`math::`/`array::`/`string::` function namespace.

Behind the `rhai` feature the crate exposes a scripting surface: a [Rhai](https://rhai.rs) engine
(registered by the `register_surreal_engine!` macro) that builds `SELECT` statements from the same
named primitives as `vantage-sql` where the two overlap, and surreal-specific ones where they don't.

## Pages

- [Primitives](./surrealdb/primitives.md) — the named expression vocabulary and how each lowers to
  SurrealQL.
