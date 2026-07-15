# Vantage Framework

Vantage is a data entity persistence and abstraction framework for Rust.

Vantage changes the way you think about your data. Instead of writing queries and loading rows,
you work with **sets** of records — "unpaid invoices older than 30 days", "customers who ordered
today" — narrowing them, combining them, traversing from one to another, and acting on them where
they live.

Vantage offers two ways to work with your data. In **transactional** mode there is no state to
manage: load a set, act on it, done — each operation goes straight to the backend. This is the
natural shape of a REST endpoint: handle the request, read or update the data, respond. (The
name describes that request/response shape, not database transactions — BEGIN/COMMIT is the
backend's business, and Vantage doesn't wrap your operations in one.) In **live** mode Vantage
maintains a local representation of the data segment you're working with: you operate on your
local copy, and it reconciles with the source over time. That's how a user interface behaves —
any kind. Live mode is built on top of transactional mode, so everything you learn about the
first carries into the second.

This documentation tracks the current **0.6** release line.

## Ethos

A few principles run through every layer of the framework:

- **Let the backend do the work.** If a database or API can filter, join, aggregate, or paginate,
  Vantage pushes the work there — data is narrowed at the source, not in your process.
- **Fill the gaps client-side, honestly.** Where a backend can't (a CSV file can't sort; two
  databases can't join), the layer above provides it — and every handle advertises exactly what it
  supports, so a missing capability is an explicit error, never a silent guess.
- **Respect native types.** Each backend keeps its own type system end to end — decimals stay
  precise, dates stay dates, nothing is funnelled through JSON.
- **Business logic lives on the entity.** Validation, audit, soft-delete, and domain methods attach
  to your model once and apply everywhere the entity is used.
- **Fail loudly, retry safely.** No panics, no silent zeros or match-alls; operations are
  idempotent wherever possible, so retrying is always an option.
- **Be aware of observers.** Data knows who is watching: changes stream to subscribers, and edits
  reconcile instead of clobbering. (This is live mode — chapters 5 through 8 build it up.)

## Three ways to work with data

Vantage lets you choose how strongly typed your data access is — per component, not per project:

1. **Entity mode** — records are plain Rust structs (`Table<SqliteDB, Product>`). Columns and
   conditions are compiler-checked, and business logic attaches to the entity: full type safety,
   the natural choice for hand-written Rust code.
2. **Record mode** — each record is an arbitrary structure of named values (`Record<V>`), with
   schema introspectable at runtime. No entity struct required — built for generic code: data
   grids, admin tools, import/export.
3. **Rhai scripts** — more generic still: schema declared in YAML, custom expressions and logic in
   [Rhai](https://rhai.rs). Defined, loaded, and sealed at runtime with no recompiling — made for
   configuration-driven and agent-driven tooling.

The three interoperate: a Rhai-declared table serves records, records deserialize into entities,
and all of it sits behind the same capability-checked handles.

## What makes Vantage different

Vantage is a **framework, not a library**. A library solves one concern and hands the rest back to
you; Vantage takes over the data layer entirely — modelling, querying, types, caching, reactivity —
and gives you well-defined places to plug in what's yours. The implementation spans 10+ crates
(query builders, backend drivers, the entity layer, the reactive stack), all built on the same
cohesive, extensible principles: what you learn in one crate applies in the next, and every
extension point looks the same wherever you meet it.

Vantage also doesn't mimic frameworks from other languages. No reflection, no runtime magic, no
inheritance hierarchies. It leans on what Rust is uniquely good at — traits for composition,
ownership for safe sharing, generics that compile away, async throughout — so the framework feels
native rather than translated.

## The four layers

Everything above maps onto four layers. Each builds on the one below, and you climb only as far as
your application needs:

```text
Table<SqliteDB, Product>   entity mode — typed, compiler-checked, transactional
Vista                      record mode — schema known at runtime, capabilities honest
Dio                        live mode begins — local cache, write queue, change events
Scenery                    reactive views over a Dio — what a UI binds to and watches
```

A `Table` is where your model and business logic live. Wrap it into a `Vista` when generic code
needs to consume it ([record mode](#three-ways-to-work-with-data), above). Bind the Vista to a `Dio` when you want a live local representation —
caching, write routing, reconciliation. Open a `Scenery` over the Dio for an ordered table, a
single record, or an aggregate that updates as the data changes. Layers never leak upward: a
`Table` doesn't know it's being cached, and a `Scenery` consumer can't tell which backend is
underneath.

## Vantage and Vantage UI

[Vantage UI](https://vantage-ui.com) is a native admin console built directly on these crates —
point it at your databases, APIs, and tools, and an AI agent configures tables, forms, and
dashboards over them. It is a closed-source product (free download), and this framework is its
open foundation: Vantage is open-sourced so that you can build your own services, CLIs, and UIs
on the same data layer — and extend it, adding persistences and capabilities that custom builds
can carry further than the stock app does.

If you want the finished tool, start with Vantage UI. If you want the foundation, read on.

## Getting Started

Vantage covers a lot of ground — multiple databases, type systems, entity frameworks, UI adapters —
but none of that matters until you've seen it do something useful.

This guide introduces Vantage concepts one at a time, each building on the last. We'll start with
something you already know — SQL — and work our way up to the bigger abstractions. Along the way we
build a small product catalog that starts as a CLI, becomes an HTTP API, and switches databases
without touching its handlers — then point the same machinery at a live cloud API and end with a
cached, reactive view of it: first in the terminal, then served over HTTP to a React frontend.

1. **[SQLite and the Query Builder](./intro/step1-first-query.md)** — connect to a database, build
   and execute typed queries, map rows to structs.
2. **[Tables and Typed Data Access](./intro/step2-tables.md)** — define entities and tables, narrow
   sets with conditions, traverse relationships, add computed fields; CRUD becomes one-liners.
3. **[A Standalone Axum Server](./intro/step3-axum-server.md)** — put the model behind HTTP with one
   generic CRUD handler for every entity, then migrate the whole server from SQLite to MongoDB by
   editing only the model.
4. **[Vista — the Universal Data Handle](./intro/step4-vista.md)** — erase the entity and backend
   into a schema-bearing runtime handle, with explicit capability contracts.
5. **[Dio & Lens — Caching and Events](./intro/step5-dio-lens.md)** — build a bucket inventory
   over a real cloud API: a persistent, resumable local cache turns a seconds-long listing into
   milliseconds, with an event bus announcing every change.
6. **[Augmentation — Enriching Rows](./intro/step6-augmentation.md)** — give every listed file
   columns computed from its contents, fetched once per file through lazy expressions and cached
   with the row.
7. **[Scenery — Reactive Views](./intro/step7-scenery.md)** — reactive views over the Dio; finish
   with a live terminal UI that scrolls 122,000 files and loads details for the rows at your
   cursor.
8. **[Serving Scenery — Axum & Watch Streams](./intro/step8-axum-dio.md)** — put the Dio behind
   HTTP: kubernetes-style GET + watch endpoints streaming augmentation to a React frontend, with
   concurrent viewers served fairly from one download per row.

You'll need basic Rust experience (structs, traits, async/await, cargo). No prior Vantage knowledge
required.

**Start here:** [SQLite and the Query Builder](./intro/step1-first-query.md)

## Beyond the guide

The rest of the book is reference material — read it when the guide points at it, or jump straight
to what you need:

- [Expressions & Queries](./expressions.md) and the per-backend chapters
  ([SQL](./sql.md), [SurrealDB](./surrealdb.md)) — the query-building layer in depth.
- [Records: Traversal, Invariants & Hooks](./record-lifecycle.md) — the write pipeline: audit
  stamps, validation, soft-delete.
- [Model-Driven Architecture](./mda.md) — how Vantage expects you to structure business software.
- [Config-Driven Vistas: YAML & Rhai](./config-driven-vistas.md) — define and reshape data handles
  from configuration, sealed at runtime without recompiling.
- [Adding a New Persistence](./new-persistence.md) — nine incremental steps to connect Vantage to
  a backend it doesn't know yet; the main path for extending the framework.
- [Augmentation](./augmentation.md) and the [Type System](./type-system.md) — enriching rows from a
  second source, and teaching backends your own types.
- [Historical Timeline](./history.md) — how the framework got here, release by release.
