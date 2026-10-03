# Writes

On a host with `Terminals::ReadWrite`, a handle also writes. Every write goes through the resolved
Vista, so it follows the backend's rules and whatever policy the host's resolver wrapped around it.

<!-- toc -->

---

## The write verbs

<!-- tested: rhai_guide::writes::write_verbs -->
```rhai
let orders = table("order");

let id = orders.insert(#{ client: "c2", total: 30, status: "due" });
orders.patch(id, #{ status: "paid" });

let deleted = orders.delete("o4");   // true: the row was there
let again = orders.delete("o4");     // false: nothing left to delete
let patched = orders.patch("o99", #{ status: "paid" });   // false

#{
    status: orders.get(id).status,
    deleted: deleted,
    again: again,
    patched: patched,
}
```

| Verb | Returns | When the row is missing |
|---|---|---|
| `insert(map)` | the new row's id | n/a |
| `upsert(id, map)` | `id` | inserts it |
| `patch(id, map)` | `true` | returns `false` |
| `delete(id)` | `true` | returns `false` |
| `import_from(source)`, `import_from(source, mapping)` | `#{ inserted, skipped, cancelled }` | n/a |

`patch` and `delete` return `false` only for a missing row: the backend reported
`ErrorKind::NotFound`. Any other failure throws. Drivers report not-found for these cases:
vantage-memory, SQL (a patch that matched no row) and SurrealDB (an update or delete that affected
nothing). A Dio Vista checks its cache and then its master before it queues the write.

`patch` changes only the fields in the map. `upsert` replaces the whole row, so fields left out of
the map are gone afterwards.

## Ids

`insert` looks at the map's id column. With a value there, that id is used as given, and an
existing row with that id is an error. Without one, the backend assigns the id and `insert`
returns it.

<!-- tested: rhai_guide::writes::explicit_ids -->
```rhai
let orders = table("order");

orders.insert(#{ id: "o10", client: "c3", total: 55, status: "due" });
orders.upsert("o10", #{ client: "c3", total: 60, status: "due" });   // replaces
orders.upsert("o11", #{ client: "c3", total: 5, status: "due" });    // inserts

table("client").where("id", "c3").ref("orders").ids()
```

Inserting an id that already exists throws:

<!-- tested: rhai_guide::writes::duplicate_insert -->
```rhai
table("order").insert(#{ id: "o1", client: "c2", total: 1, status: "due" })
```

Use `upsert` when a script owns stable ids and may run more than once.

## Narrowing doesn't filter writes

A write goes to the handle's table. `where`, `sort`, `search` and `limit` decide what reads
return, not which rows a write may touch:

<!-- tested: rhai_guide::writes::writes_ignore_narrowing -->
```rhai
let paid = table("order").where("status", "paid");
paid.delete("o2");   // o2 is due, and is deleted all the same

let ada = table("client").where("id", "c1");
ada.ref("orders").insert(#{ id: "o20", client: "c1", total: 9, status: "due" });

#{
    orders: table("order").ids(),
    clients: table("client").count(),
}
```

The exception is `ref`: after a `ref` step, writes go to the relation's target table. Steps before
the last `ref` are read (to find the related rows); steps after it are ignored for writes.

`insert` through a `ref` doesn't fill the foreign key. Set it in the map, as above.

## Capabilities

Each write checks the target Vista's capabilities before it runs, and throws an error naming the
verb and the table when the backend can't do it:

| Verb | Needs |
|---|---|
| `insert` | `can_insert` |
| `upsert` | `can_insert` and `can_update` |
| `patch` | `can_update` |
| `delete` | `can_delete` |
| `import_from` | `can_import`, or `can_insert` for row-by-row inserts |

A host can also turn writes off for every table with `Writes::Denied(message)`. The verbs are
still there, and each throws `message`. Vantage UI does this for MCP agents unless the "Allow MCP
agents to write data" setting is on.

## Importing

`import_from(source)` copies every row of another handle into this table. `source` is read through
its narrowing, so it can be any set: a filtered table, the target of a `ref`, a table on another
datasource.

With a mapping, each imported row is built from a source row. String values in the mapping may
reference source columns as `${row.<col>}`:

<!-- tested: rhai_guide::writes::import_mapped -->
```rhai
let report = table("archive").import_from(
    table("order").where("status", "paid"),
    #{ id: "a-${row.id}", amount: "${row.total}", note: "paid by ${row.client}" }
);

#{ report: report, rows: table("archive").list() }
```

- A value that is exactly one reference (`"${row.total}"`) copies the column's value with its
  type: `amount` stays an integer.
- Any other string interpolates into text (`"paid by ${row.client}"`).
- Non-string values are literals.
- The mapping must set the target's id column. Two source rows mapping to the same id are an
  error, and so is an id whose `table:` prefix names a different table.

When the target can import (`can_import`), the rows go through `import_values` in one call, and
rows the target already holds count as `skipped`. Otherwise each row is inserted, and an existing
id throws. Importing the same rows twice into a memory table:

<!-- tested: rhai_guide::writes::import_twice -->
```rhai
let first = table("backup").import_from(table("client"));
let second = table("backup").import_from(table("client"));

[first.inserted, second.inserted, second.skipped]
```

returns `[3, 0, 3]`.

`cancelled` is `true` when the backend stopped part-way: a host's import can be cancellable (Vantage
UI shows a progress dialog with a cancel button). `inserted` then counts the rows written before
the stop. A backend signals this by returning an error carrying the
[`IMPORT_CANCELLED`](vantage_vista::rhai::IMPORT_CANCELLED) context key.

## Writes through a Dio

A host may hand scripts Vistas whose writes go through a Dio: `Dio::vista()` is a Vista over the
Dio's cache whose inserts, patches and deletes are queued as flashes and written through to the
master. To the script it is just a Vista. It is how action bodies in Vantage UI write: the page
sees the change at once, and the master write follows.

An `insert` without an id on a Dio Vista goes straight to the master (there is no id to stage
until the master assigns one) and then seeds the cache, so an immediate `get(id)` finds the row.
