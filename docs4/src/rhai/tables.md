# The Table Handle

`table(name)` returns a handle: a table plus an ordered list of narrowing steps. Narrowing verbs
add steps. Terminal verbs resolve the handle into a Vista, apply the steps, and read or write.

<!-- toc -->

---

## Narrowing

<!-- tested: rhai_guide::tables::narrowing -->
```rhai
let paid = table("order").where("status", "paid");
let big = paid.where("total", ">", 100);
let top = paid.sort("total", "desc").first();

#{
    paid: paid.count(),
    big: big.ids(),
    top: top.id,
}
```

Each narrowing verb returns a **new** handle and leaves the one it was called on alone. `big` and
the sorted handle both start from `paid`, and `paid.count()` still counts every paid order. You can
keep a base handle in a variable and branch from it as often as you like.

| Verb | Meaning |
|---|---|
| `where(col, value)` | rows whose `col` equals `value` |
| `where(col, op, value)` | `op` is `=`/`==`/`eq`, `!=`/`ne`, `>`/`gt`, `>=`/`gte`, `<`/`lt`, `<=`/`lte`, `in`, `not_in` or `like`; `in` and `not_in` take an array |
| `sort(col)`, `sort(col, "asc" \| "desc")` | order by `col`; the column must be flagged orderable |
| `search(text)` | the backend's quicksearch |
| `limit(n)` | at most `n` rows, the first `n` in sort order; `n` must be above 0 |
| `offset(n)` | the rows after the first `n` in sort order, as SQL's `OFFSET`; `n` can't be negative |
| `ref(relation)` | the related rows of every row in the set |

Conditions add up: every `where` narrows further. Sorting keeps one column. A later `sort`
replaces an earlier one, as `Vista::set_order` does. Several `limit` steps keep the smallest, and
several `offset` steps add up, so a later step never widens the set.

`offset` and `limit` combine as in SQL, whatever order you call them in: `offset(20).limit(10)`
and `limit(10).offset(20)` are both rows 20 to 29. The offset is skipped first and the limit counts
from there.

`limit(n)` and `offset(n)` pick from the set itself, not just one read: `list()`, `ids()` and
`count()` all see the same rows, a `ref` after them follows only those rows, and `import_from`
copies only those (see [Following relations](#following-relations)). They don't carry over to the
target of a `ref`; limit or offset the target again if you need to. A handle with `limit` or
`offset` reads; it can't be written through ([Writes](./writes.md#writes-stay-in-the-set)).

The column in `where` and `sort` must be one the backend stores. A
[computed column](./computed.md) is refused with an error naming it.

Nothing is checked while you narrow. A step the resolved Vista can't apply (an unknown relation, a
column that isn't orderable, `search` on a backend without search) fails at the terminal verb:

<!-- tested: rhai_guide::tables::late_error -->
```rhai
let invoices = table("client").ref("invoices");   // nothing resolves yet
invoices.count()                                  // throws here
```

The error names the step, `Step can't be applied (step: Ref("invoices"))`, and its cause, an
`unknown relation` naming `invoices`. An unknown relation is a not-found error.

## Reads

<!-- tested: rhai_guide::tables::reads -->
```rhai
let clients = table("client");
let ada = clients.get("c1");
let ghost = clients.get("c9");

#{
    name: ada.name,
    missing: ghost == (),
    first_two: clients.sort("name").limit(2).list().map(|c| c.name),
}
```

| Verb | Returns |
|---|---|
| `list()` | an array of row maps, capped by the host's limit and any `limit(n)`, after any `offset(n)` |
| `get(id)` | the row map, or `()` when no row has that id |
| `first()` | the first row map in sort order (after `offset(n)`, the row at position `n`), or `()` for an empty set |
| `count()` | the number of rows `list()` would return without the host's limit, as an integer: at most `n` after `limit(n)`, `n` fewer (never below 0) after `offset(n)` |
| `ids()` | an array of id strings, in sort order (insertion order when unsorted) |

Row maps always include the id column, filled from the row's key when the backend doesn't return
it as a field. Ids are strings. `get`, and every verb that takes an id, also accepts an integer
and uses its decimal form.

A missing row is not an error for reads: `get` returns `()`. Compare with `()` as above, or use
Rhai's `??`.

A capped read (the host's limit or a `limit(n)`) reads the set's leading window,
`fetch_window(0, n)`, which every backend that lists answers, so it fetches only the rows it
returns. A backend that refuses the leading window makes the read throw; it never falls back to
listing the whole set.

With an `offset(n)` too, a backend advertising `window` reads `fetch_window(n, limit)`, so the
skipped rows never leave it. Any other backend reads the leading window `fetch_window(0, n +
limit)` and drops the first `n` rows. An offset with no cap at all (no `limit` and no host limit)
lists the whole set and drops the first `n` rows, since there is no window to ask for. An offset
past the end reads nothing: `list()` is empty, `first()` is `()` and `count()` is 0.

## Following relations

`ref(relation)` works on sets, not single rows. The result is every row related to any row of the
current set:

<!-- tested: rhai_guide::tables::ref_sets -->
```rhai
// The orders of one client...
let ada_orders = table("client").where("id", "c1").ref("orders");
// ...and the clients behind every due order.
let owing = table("order").where("status", "due").ref("client");

#{
    ada: ada_orders.ids(),
    owing: owing.sort("name").list().map(|c| c.name),
}
```

Relations come from the Vista's metadata: the YAML `references:` block, or the typed table's
`with_one` / `with_many`. A `ref` step reads the rows it starts from when the handle resolves:

- When the set has exactly one row, the step calls `Vista::get_ref(relation, row)`, so a backend's
  own traversal applies (SurrealDB record links, scripted references, cross-datasource
  references).
- When it has several, the step takes the bare target and adds an `in` condition: on the foreign
  key for has-many (the target rows point at ours), on the target's id for has-one (our rows point
  at the target).
- When it has none, or none of the rows has a has-one key, the target is an empty set. Nothing is
  queried: reads return nothing, `count()` is 0, and a preview reports `"query": null`. The target
  keeps its columns, and an insert into it throws: no row can belong to an empty set.
- A `limit(n)` before the `ref` caps the rows it follows, and an `offset(n)` skips the first `n`
  of them. Without a limit, a `ref` over more than 1,000 rows (after the offset) is an error.
  Narrow first.

<!-- tested: rhai_guide::tables::limit_and_ref -->
```rhai
let biggest = table("order").sort("total", "desc").limit(2);
let nobody = table("client").where("name", "Nobody");

#{
    count: biggest.count(),
    clients: biggest.ref("client").sort("name").list().map(|c| c.name),
    none: nobody.ref("orders").count(),
}
```

The two biggest orders are `o1` (Ada) and `o3` (Ben), so this returns `count: 2`, `clients:
["Ada", "Ben"]` and `none: 0`.

Steps after `ref` narrow the target, so `ref("orders").where("status", "due")` is the due orders
of the set. A relation keyed on a [computed column](./computed.md#relations-through-a-computed-column)
works for has-one and is refused for has-many.

## Introspection

Three reads describe the table instead of its rows:

<!-- tested: rhai_guide::tables::introspection -->
```rhai
let orders = table("order");

#{
    insert: orders.capabilities().insert,
    columns: orders.columns().map(|c| c.name),
    references: orders.references().map(|r| r.name + " " + r.kind),
}
```

This returns `insert: true`, the columns `id`, `client`, `total`, `status`, and the references
`["client has_one"]`.

- `capabilities()` is a map with one boolean per capability, keyed by its name. The keys come
  from [`Capability::iter()`](vantage_vista::Capability::iter), so every capability is listed:
  `insert`, `update`, `delete`, `import`, `subscribe`, `order`, `search`, `filter_ops`,
  `native_conditions`, `window`, `cursor`, `traverse` and `confine_writes`. There is no `count`
  key: every Vista can count. A script that may run against different backends can check before
  it writes.
- `columns()` is an array of `#{ name, type, flags }`. `type` is `()` for a column declared
  without a type.
- `references()` is an array of `#{ name, kind, contained }`. `kind` is the YAML name:
  `"has_one"`, `"has_many"`, `"contains_one"` or `"contains_many"`; `contained` marks relations
  stored inside the row (see
  [Contained Relations](../new-persistence/step9-contained-relations.md)).

These resolve the handle, so they describe the table as narrowed. Capabilities rarely change with
narrowing, but a query-sourced table reports no write capabilities at all.

## Handles in Rust

A host that gets a handle back from a script reads it through
[`Handle`](vantage_vista::rhai::Handle): `table_name()` is the starting table (`None` for a handle
over a Vista), and `resolve(resolver)` builds the Vista with every step applied. Query preview works
this way: the script returns a handle, and the host renders the resolved Vista's query.

A handle can also start from a Vista instead of a name, which is how `self` reaches `modify:`
scripts. The handle holds that Vista as its base and clones it on each resolve, so the same handle
resolves as often as you like. Verbs on `self` return new handles, like any other narrowing.
