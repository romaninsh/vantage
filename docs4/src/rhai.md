# Scripting with Rhai

Vantage applications keep much of their behaviour in configuration: YAML table specs, page
definitions, action bodies, faker sims. When that configuration needs logic (a filter that depends
on a row, a write after a form is submitted, a simulated order that ships after twenty minutes) it
is written in [Rhai](https://rhai.rs), a small scripting language embedded in Rust.

This part of the book is the one place that teaches scripting across the framework. Other chapters
link here instead of repeating the verbs.

<!-- toc -->

---

## A first script

Every data script uses the same handful of words. `table(name)` names a table, a chain of
narrowing verbs describes a set of its rows, and a terminal verb reads or writes:

<!-- tested: rhai_guide::tables::first_script -->
```rhai
let vips = table("client").where("vip", true);
let due = vips.ref("orders").where("status", "due");

#{
    vips: vips.count(),
    due: due.ids(),
}
```

`vips` and `due` are **handles**: descriptions of a set. Nothing is read until `count()` or
`ids()` runs, and the handle is then resolved into a [`Vista`](vantage_vista::Vista) and read
through it. `ref("orders")` follows a relation from every row of the set, so `due` is the due orders
of every VIP client. The script's last expression is its result; here a map, which a host can turn
into JSON.

The script never mentions SQL, SurrealDB or HTTP. The same script runs over a SQLite table, a
SurrealDB table, a REST API or an in-memory store, because the words act on Vistas, and every
backend can be wrapped as a Vista.

## Where scripts run

A script always runs inside a **host**: a Rhai engine with fixed operation limits, a compile cache,
and the vocabularies the host chose to register. The host decides what `table(name)` resolves to,
whether writes are allowed, and how many rows a `list()` may return. The same script text can be
legal in one place and refused in another.

| Surface | Example | What the script can do |
|---|---|---|
| Agent data scripts (`run_script`) | an MCP tool reading or fixing data | read, and write if the host allows it |
| Query preview (`preview_script`) | an MCP tool showing the query a script would run | describe a set, never read it |
| YAML `modify:`, reference build scripts, augmentation sources | `self.where("vip", true)` | describe a set |
| Form `options:` | a dropdown filled from another table | read |
| Action bodies, form `on_submit`, wizard workers | `row.status = "paid"; row.save();` | read and write |
| Faker sims | an order that ships, then disappears | read and write a memory store, plus time and random verbs |

[Hosts](./rhai/hosts.md) explains how a host is put together, and [Surfaces](./rhai/surfaces.md)
walks through each row of this table with a working example.

## The layers

Vista is the universal data handle: any backend, one type-erased interface. The data vocabulary
lives in `vantage-vista` (behind its `rhai` feature) and knows nothing except Vistas. Three things
sit above Vista, and each has its own scripting words:

```text
  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐
  │ Servo        │  │ Scenery      │  │ faker sims       │
  │ form drafts  │  │ view shapes  │  │ time, random,    │
  │ (diorama)    │  │ (vantage-ui) │  │ fake_row (faker) │
  └──────┬───────┘  └──────┬───────┘  └────────┬─────────┘
         │                 │                   │
  ┌──────┴─────────────────┴───────────────────┴─────────┐
  │ Vista data vocabulary: table, where, sort, ref,      │
  │ list, get, insert, patch, record, ...                │
  └──────────────────────────────────────────────────────┘
```

The lower layer never refers to the upper ones. A Dio can hand a script a Vista whose writes go
through its write queue, and the script can't tell the difference. Servo and Scenery keep their own
words on purpose, because they describe different things: a form draft bound to one record, and the
shape of a live view. [Layers](./rhai/layers.md) covers how they relate and which words they share.

There is also a second kind of script: **expressions** that build a backend's native query, such as
a SQL `select()` or a SurrealDB condition. They aren't data scripts and have their own chapters.
[Expression dialects](./rhai/dialects.md) points to them.

## What this part covers

- [Hosts](./rhai/hosts.md): `Host`, `Vocab`, `Limits`, the `DataVocab` configuration, the resolver
  behind `table(name)`, and how async reads run from synchronous scripts.
- [The table handle](./rhai/tables.md): narrowing verbs, reads, relations, introspection, and
  errors.
- [Writes](./rhai/writes.md): `insert`, `upsert`, `patch`, `delete` and `import_from`, and what
  each returns when a row is missing.
- [Records](./rhai/records.md): drafts of one row that stage edits and save only what changed.
- [Surfaces](./rhai/surfaces.md): every place the vocabulary runs, with a tested example for each,
  plus backend extensions such as SurrealDB's `with_condition`.
- [Layers](./rhai/layers.md): Servo, Scenery and faker sims next to the data vocabulary.
- [Expression dialects](./rhai/dialects.md): query builders, `lazy:` columns and templates.

```admonish note title="Tested examples"
Every script in this part is copied from a test. The comment above a snippet names the test, for
example `rhai_guide::tables::first_script` in `vantage-vista/tests/rhai_guide/`. Run them with
`cargo nextest run -p vantage-vista --all-features -E 'binary(rhai_guide)'`.
```

## Words at a glance

| Group | Words |
|---|---|
| Start | `table(name)`; `self` in slot scripts; `table()` in faker sims |
| Narrow | `where(col, value)`, `where(col, op, value)`, `sort(col)`, `sort(col, dir)`, `search(text)`, `limit(n)`, `ref(relation)` |
| Read | `list()`, `get(id)`, `first()`, `count()`, `ids()`, `columns()`, `references()`, `capabilities()` |
| Write | `insert(map)`, `upsert(id, map)`, `patch(id, map)`, `delete(id)`, `import_from(source)`, `import_from(source, mapping)` |
| Record | `record()`, `record(id)`; on a record: `r.col`, `r["col"]`, `set(map)`, `id`, `is_dirty()`, `dirty(col)`, `baseline()`, `revert()`, `revert(col)`, `save()`, `delete()`, `status()`, `rejection()` |

`where` takes the operators `=` / `==` / `eq`, `!=` / `ne`, `>` / `gt`, `>=` / `gte`, `<` / `lt`,
`<=` / `lte`, `in`, `not_in` and `like`.
