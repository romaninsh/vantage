# SurrealDB

The `vantage-surrealdb` crate targets [SurrealDB](https://surrealdb.com) — a document-graph database
whose query language (SurrealQL) is close enough to SQL to share a vocabulary, but diverges where it
matters: graph traversals instead of joins, embedded arrays with closures, record links, and a
`math::`/`array::`/`string::` function namespace.

Behind the `rhai` feature the crate exposes a scripting surface: a [Rhai](https://rhai.rs) engine
(registered by the `register_surreal_engine!` macro) that builds `SELECT` statements from the same
named primitives as `vantage-sql` where the two overlap, and surreal-specific ones where they don't.

## The Vista

With the `vista` feature, `SurrealVistaFactory` builds a [Vista](./intro/step4-vista.md) from a
YAML spec. It advertises the reads SurrealQL can do in the query: `Order`, `Search`, `FilterOps`,
`NativeConditions` (the `with_condition` verb in `modify:` scripts), `Window` (`LIMIT … START …`,
served by `fetch_window`) and `Traverse`. A table-sourced Vista adds `Insert`, `Update`, `Delete`,
`ConfineWrites` and `Subscribe`, the last through a `LIVE SELECT`. A query-sourced one (`rhai:` or
`base:`) has no table to write to or to subscribe to, so it advertises the reads only.

A quicksearch reads the columns flagged `searchable`, or every column when none is flagged: the
same rule as every other driver (`Table::search_columns`). Each column is matched as a
case-insensitive substring, `string::contains(string::lowercase(<string>(col)), $text)` with the
text lowercased, and the matches are joined with `OR`. A computed (`expr:`) column is searched
through its expression, because its alias doesn't exist where `WHERE` runs. Imported reference
columns are left out.

## Pages

- [Primitives](./surrealdb/primitives.md) — the named expression vocabulary and how each lowers to
  SurrealQL.
