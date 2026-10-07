# Scripting with Rhai

A Vantage application can keep much of its behaviour in configuration: YAML table specs, page
definitions, action bodies, faker sims. Some of that configuration needs logic: a column derived
from other columns, a filter that depends on the selected row, a write after a form is submitted,
a simulated order that ships after twenty minutes. That logic is written in
[Rhai](https://rhai.rs), a small scripting language embedded in Rust.

Rhai is part of vantage: any application that holds Vistas can run these scripts. Vantage UI is one
such application and is used as an example where it helps.

This part of the book is the one place that teaches scripting across the framework. Other chapters
link here instead of repeating the verbs.

<!-- toc -->

---

## Why scripts

Configuration is data. A user, an agent or a hot-reloading inventory can change it while the
application runs, and nothing gets recompiled. Rust code can't follow it there, so the logic that
travels with the configuration has to be data too: text that the application compiles and runs
when it loads the YAML.

Rhai fits that job:

- **It runs in the process.** There is no interpreter to install and no subprocess. A script
  calls straight into the same Vistas the Rust code uses.
- **It can only do what the host allows.** A Rhai script has no file system, network or process
  access of its own. It sees the functions the host registered and nothing else.
- **It can't hang the caller.** Every host caps the number of operations a script may run. A
  script that loops forever fails with an error.
- **It is backend-neutral.** Data scripts act on [Vistas](./intro/step4-vista.md), so the same
  script runs over SQLite, SurrealDB, a REST API or an in-memory store.

## What a script can do

| Task | Example | Chapter |
|---|---|---|
| Read a set of rows, count it, follow a relation | `table("client").where("vip", true).ref("orders").count()` | [The table handle](./rhai/tables.md) |
| Insert, patch, delete, copy rows between tables | `table("order").patch(id, #{ status: "paid" })` | [Writes](./rhai/writes.md) |
| Compute a column from the other columns of a row | `lazy: "row.net + row.vat"` | [Computed columns](./rhai/computed.md) |
| Edit one row field by field and save what changed | `row.status = "paid"; row.save();` | [Records](./rhai/records.md) |
| Narrow a table in YAML, or build a relation's target | `self.where("vip", true)` | [Surfaces](./rhai/surfaces.md) |
| Build a backend's native query | `select().from("product").field("id")` | [Expression dialects](./rhai/dialects.md) |

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
| YAML `lazy:` columns | `row.net / 5` | compute one value from one row |
| An application's form `options:` (e.g. Vantage UI) | a dropdown filled from another table | read |
| An application's action bodies, form `on_submit`, wizard workers (e.g. Vantage UI) | `row.status = "paid"; row.save();` | read and write |
| Faker sims | an order that ships, then disappears | read and write a memory store, plus time and random verbs |

[Hosts](./rhai/hosts.md) explains how a host is put together, and [Surfaces](./rhai/surfaces.md)
walks through each row of this table with a working example.

## Rhai in two minutes

Rhai reads like a mix of Rust and JavaScript. The parts these chapters use:

| Rhai | Meaning |
|---|---|
| `let x = 5;` | a variable; no type annotations |
| `#{ name: "Ada", vip: true }` | an object map; read with `m.name` or `m["name"]` |
| `[1, 2, 3]` | an array |
| `\|c\| c.name` | a closure, as in `rows.map(\|c\| c.name)` and `rows.filter(\|c\| c.vip)` |
| `()` | "nothing": a missing field, a row that wasn't found |
| `a ?? b` | `a`, or `b` when `a` is `()` |
| `` `total: ${t}` `` | string interpolation |
| `if c { a } else { b }` | an expression; it has a value |
| `try { … } catch (err) { … }`, `throw "message"` | catching and raising errors |

A script's value is its last expression, with no `return` and no trailing `;`. Methods chain, so
`table("order").where("status", "due").count()` is three calls on the result of the one before.
The [Rhai book](https://rhai.rs/book/) covers the full language.

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

The script never mentions SQL, SurrealDB or HTTP. The words act on Vistas, and every backend can be
wrapped as a Vista.

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
  each returns when a row is missing or outside the set.
- [Computed columns](./rhai/computed.md): `lazy:` columns that the Vista fills on every read, and
  what they refuse.
- [Records](./rhai/records.md): drafts of one row that stage edits and save only what changed.
- [Surfaces](./rhai/surfaces.md): every place the vocabulary runs, with a tested example for each,
  plus backend extensions such as SurrealDB's `with_condition`.
- [Layers](./rhai/layers.md): Servo, Scenery and faker sims next to the data vocabulary.
- [Faker sims](./rhai/faker.md): scripts that keep a memory store changing over sim time.
- [Expression dialects](./rhai/dialects.md): query builders, templates and command scripts.
- [Tables defined by a query](./rhai/query-tables.md): a table whose source is a script-built
  `SELECT`, and why that is the only place a script builds a query.
- [Query builder functions](./rhai/query-builders.md): every function the SQL and SurrealDB query
  builders register.

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
| Computed column | `row` (the record as read); the last expression is the value |

`where` takes the operators `=` / `==` / `eq`, `!=` / `ne`, `>` / `gt`, `>=` / `gte`, `<` / `lt`,
`<=` / `lte`, `in`, `not_in` and `like`, plus a few aliases (`<>`, `nin`, `contains`). `sort` takes `"asc"` or `"desc"` (or `"ascending"` /
`"descending"`).
