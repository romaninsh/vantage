# Records

A record is a draft of one row. It reads like a map, stages edits, and flashes only what changed.
Use it when a script edits a row field by field, or when a host hands a script "the current row"
(Vantage UI's action bodies receive `row` as a record).

<!-- toc -->

---

## Editing an existing row

`record(id)` loads the row through the handle and throws if no row has that id:

<!-- tested: rhai_guide::records::edit_existing -->
```rhai
let o = table("order").record("o2");
o.status = "paid";
o["total"] = 45;

let was = o.baseline().status;
let staged = [o.dirty("status"), o.dirty("client")];
o.flash();   // patches `status` and `total`, nothing else

#{
    was: was,
    staged: staged,
    dirty: o.is_dirty(),
    stored: table("order").get("o2"),
}
```

A record keeps two layers:

- the **baseline**: the row as last loaded or flashed (`baseline()` returns it as a map);
- the **changes**: fields set since then.

Reading a field (`o.status`, `o["status"]`) returns the change if there is one, else the baseline
value, else `()`. Setting a field stages a change. `flash()` sends a `patch` with the changed fields
only, then folds them into the baseline. A record with no changes writes nothing and returns its id.

## A new row

`record()` with no id starts a draft of a row that doesn't exist yet:

<!-- tested: rhai_guide::records::new_row -->
```rhai
let n = table("client").record();
n.set(#{ name: "Dee", vip: false });
let id = n.flash();   // an insert under a new UUIDv7 id

#{
    same_id: n.id == id,
    status: n.status(),
    stored: table("client").get(id).name,
}
```

The first `flash()` of a new record is an `insert`. After it, the record has an id and later
flashes are patches. `set(map)` stages several fields at once and returns the record, so
`table("t").record().set(#{ … }).flash()` works in one line. A key with dots (`"inventory.stock"`)
writes into a nested map, which is how a form field bound to an embedded object reaches the right
place.

The id column can't be set on a record. `n.id` is read-only, and setting the id column through
`set` or an index throws:

<!-- tested: rhai_guide::records::id_is_read_only -->
```rhai
let n = table("client").record();
n.set(#{ id: "c9", name: "Eve" })   // throws: `id` is the id column
```

To insert under an id you choose, use `insert(#{ id: …, … })` or `upsert(id, map)` on the handle.

The record finds out which column is the id column on the first field set, by resolving its
table, and keeps the answer, so later sets don't resolve the table again. A table with no id
column falls back to `id`.

## Dirty state and revert

<!-- tested: rhai_guide::records::revert -->
```rhai
let c = table("client").record("c2");
c.name = "Benjamin";
c.vip = true;

c.revert("vip");
let still_dirty = c.is_dirty();   // `name` is still staged
c.revert();

[still_dirty, c.is_dirty(), c.name]
```

| Verb | Meaning |
|---|---|
| `is_dirty()` | any field staged |
| `dirty(col)` | `col` staged |
| `revert(col)` | drop the staged change to `col` |
| `revert()` | drop every staged change |
| `baseline()` | the last flashed row, as a map (empty for a new record) |

## Status and failures

`status()` is `"tracking"` normally, `"pending"` while `flash()` or `delete()` runs, and `"failed"`
when the last one failed. A failed flash throws, and the record keeps its staged changes, so the
script can fix them and try again. `rejection()` then returns `#{ message }`, and returns `()`
while the record isn't failed.

<!-- tested: rhai_guide::records::failed_flash -->
```rhai
let o = table("order").record("o3");
table("order").delete("o3");   // the row goes away underneath the draft

o.status = "refunded";
let threw = false;
try { o.flash(); } catch (err) { threw = true; }

#{
    threw: threw,
    status: o.status(),
    message: o.rejection().message,
    staged: o.dirty("status"),
}
```

Flashing a record whose row has gone fails with "record `o3` no longer exists". Servo drafts, which
carry per-field validation errors from a Dio's flash route, also report a `fields` map in their
rejection (see [Layers](./layers.md)).

A new record mints its UUIDv7 on the first `flash()` and keeps it, so flashing again after a
failure can't create a second row (see [Safe writes](../record-lifecycle.md#safe-writes)). A table
whose id column is flagged `auto` leaves the id to the backend instead.

## Deleting

`delete()` deletes the record's row and returns `true`, also when the row was already gone. A
delete that isn't allowed throws, and the record is marked `failed` with the error as its
rejection. A new record that was never flashed has nothing to delete and throws "record has no id;
it was never flashed".

## Permissions

Records write through the same checks as the write verbs: the target's capabilities (`insert` for
a new record, `update` for an existing one, `delete` for `delete()`), and the host's `Writes`
setting. Under `Writes::Denied(message)`, `record(id)` still loads and reads, and `flash()` or
`delete()` throws `message`. Vantage UI uses this for action predicates (`when:`), which see `row`
but must not change it.

## Records in Rust

A host that already holds a row can give it to a script without a second fetch:
[`RecordDraft::from_row(handle, writes, id, row)`](vantage_vista::rhai::RecordDraft::from_row).
A named handle resolves through the resolver it carries, set with
[`Handle::with_resolver`](vantage_vista::rhai::Handle::with_resolver).
After the script runs, `changes()` returns what it staged and `values()` the row as the script
last saw it. Vantage UI builds the action body's `row` this way, over the page's Dio Vista.

A row that belongs to no table, such as the `row` an action predicate reads, still needs a
handle with a resolver: the first field set asks it for the id column. A Vista over an empty
[`MockShell`](vantage_vista::mocks::MockShell) (feature `mocks`) serves:

<!-- tested: rhai_guide::records::row_without_table -->
```rust,ignore
let no_table: Arc<dyn VistaResolver> =
    Arc::new(|_: &str| Ok(Vista::new("row", MockShell::new())));
let row = RecordDraft::from_row(
    Handle::named("row").with_resolver(no_table),
    Writes::Denied("this row is read-only".into()),
    "o1".into(),
    values,
);
```

A script given this `row` can read it and stage fields, and every flash throws the denial:

<!-- tested: rhai_guide::records::row_without_table -->
```rhai
row.status = "void";   // staged on the draft
let flashed = true;
try { row.flash(); } catch (err) { flashed = false; }

#{ status: row.status, was: row.baseline().status, flashed: flashed }
```

returns `status: "void"`, `was: "paid"` and `flashed: false`. Setting `row["id"]` still throws,
because the empty table has no id column and the draft falls back to `id`.
