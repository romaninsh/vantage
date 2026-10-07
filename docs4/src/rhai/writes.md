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
let again = orders.delete("o4");     // true: the row is gone either way
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
| `insert(map)` | the id; an id already in the set returns it | n/a |
| `upsert(id, map)` | `id` | inserts it |
| `patch(id, map)` | `true` | returns `false` |
| `delete(id)` | `true`; `false` only when writes aren't allowed | returns `true` |
| `import_from(source)`, `import_from(source, mapping)` | `#{ inserted, skipped, rejected, cancelled }` | n/a |

`patch` returns `false` when the row is missing or outside the handle's set (the backend reported
`NotFound`). `delete` returns `true` whenever the row is gone afterwards, including when it was
never there or is outside the set; it returns `false` only when the write isn't allowed
(`Writes::Denied`, no `can_delete`, a backend that can't keep writes inside the set). Any other
failure throws. Why: [Safe writes](../record-lifecycle.md#safe-writes).

`patch` changes only the fields in the map. `upsert` replaces the whole row, so fields left out of
the map are gone afterwards. [Computed columns](./computed.md) in the map are dropped before the
write reaches the backend.

## Ids

`insert` looks at the map's id column. With a value there, that id is used as given. Without one,
`insert` mints a UUIDv7 before the first attempt, so a retry reuses it, and returns it. When the
id column is flagged `auto`, the backend makes the id instead (`insert_return_id`, not
retry-safe; see [Safe writes](../record-lifecycle.md#safe-writes)). A numeric id column without
`auto` is an error: pass an id or flag the column.

<!-- tested: rhai_guide::writes::explicit_ids -->
```rhai
let orders = table("order");

orders.insert(#{ id: "o10", client: "c3", total: 55, status: "due" });
orders.upsert("o10", #{ client: "c3", total: 60, status: "due" });   // replaces
orders.upsert("o11", #{ client: "c3", total: 5, status: "due" });    // inserts

table("client").where("id", "c3").ref("orders").ids()
```

Inserting an id that already exists in the set returns it; an id held by a row outside the set
throws. This insert returns `"o1"` and changes nothing:

<!-- tested: rhai_guide::writes::insert_existing_id -->
```rhai
table("order").insert(#{ id: "o1", client: "c2", total: 1, status: "due" })
```

Use `upsert` when a script owns stable ids and may run more than once.

## Writes stay in the set

`where`, `search` and `ref` define the set; a write only touches rows in it, an insert fills the
set's equality conditions (including a `ref`'s foreign key), and a write that would leave the set
throws. `sort` doesn't change membership. A handle with `limit(n)` can't be written through: drop
the limit.

<!-- tested: rhai_guide::writes::writes_stay_in_the_set -->
```rhai
let paid = table("order").where("status", "paid");
let o2 = paid.delete("o2");                         // o2 is due: outside the set
let o9 = paid.patch("o2", #{ total: 1 });           // false: not in the set

let ada = table("client").where("id", "c1");
ada.ref("orders").insert(#{ id: "o20", total: 9, status: "due" });   // client filled

#{
    orders: table("order").ids(),
    o20: table("order").get("o20").client,
    o2: o2,
    o9: o9,
}
```

returns `#{ orders: ["o1", "o2", "o3", "o4", "o20"], o20: "c1", o2: true, o9: false }`: `o2` is
still there.

## Capabilities

Each write checks the target Vista's capabilities before it runs, and throws an error naming the
verb and the table when the backend can't do it (`delete` returns `false` instead):

| Verb | Needs |
|---|---|
| `insert` | `can_insert` |
| `upsert` | `can_insert` and `can_update` |
| `patch` | `can_update` |
| `delete` | `can_delete` |
| `import_from` | `can_import`, or `can_insert` for row-by-row inserts |

A host can also turn writes off for every table with `Writes::Denied(message)`. The verbs are
still there, and each throws `message`, except `delete`, which returns `false`. Vantage UI does
this for MCP agents unless the "Allow MCP agents to write data" setting is on.

## Importing

`import_from(source)` copies every row of another handle into this table. `source` is read through
its narrowing, so it can be any set: a filtered table, the target of a `ref`, a table on another
datasource. A `limit(n)` on the source copies only its first `n` rows:

<!-- tested: rhai_guide::writes::import_limited -->
```rhai
let biggest = table("order").sort("total", "desc").limit(2);
let report = table("archive").import_from(biggest);

[report.inserted, table("archive").ids()]
```

returns `[2, ["o1", "o3"]]`. Without a mapping, each row keeps its id.

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

An id the target already holds is never overwritten; it counts as `skipped`. Rows that conflict
with a narrowed target count as `rejected`. When the target can import (`can_import`) and isn't
narrowed, the rows go through `import_values` in one call and the backend decides which ids it
already holds. Otherwise each row is looked up with `get` first and inserted on its own if it is
missing. Into a narrowed target, a row outside the set is `rejected` and the import carries on;
any other insert failure stops the import and throws. Importing the same rows twice into a memory
table:

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
[`IMPORT_CANCELLED`](vantage_vista::rhai::IMPORT_CANCELLED) context key (the rows inserted), and
optionally [`IMPORT_CANCELLED_SKIPPED`](vantage_vista::rhai::IMPORT_CANCELLED_SKIPPED) (the rows
skipped).

## Writes through a Dio

A host may hand scripts Vistas whose writes go through a Dio: `Dio::vista()` is a Vista over the
Dio's cache whose inserts, patches and deletes are queued as flashes and written through to the
master. To the script it is just a Vista. It is how action bodies in Vantage UI write: the page
sees the change at once, and the master write follows.

An `insert` without an id is queued like any other, with its minted UUIDv7. When the id column is
flagged `auto`, it goes straight to the master instead (there is no id to stage until the master
assigns one) and then seeds the cache, so an immediate `get(id)` finds the row.
