# Step 9: Contained Relations

Some data doesn't live in its own table. A product carries an `inventory` object; an order carries
a `lines` array. The records are real — they have fields, you want to list them, add to them, edit
one — but they're physically embedded in a column of the parent row, not stored in a table of their
own.

`with_one` / `with_many` (Step 6) can't model this: they resolve to *another* table via a foreign
key. A contained relation resolves to a **sub-`Vista` backed by one column of the same row**.
Reads project that column into records; writes patch the column back in place. To the consumer it
looks exactly like any other relation — `get_ref("lines")`, then `list_values` / `insert` / `patch`
/ `delete`.

This step is optional. Skip it unless your backend stores embedded objects or arrays that users
should be able to edit as records.

### What the framework gives you

Almost all of it is backend-agnostic and already done:

- **Declaration** is on the typed `Table`, mirroring `with_one`:

  ```rust
  Table::new("order", db)
      .with_id_column("id")
      .with_column_of::<…>("lines")          // declare the host column (see Sharp edges)
      .with_contained_many("lines", "lines", |db| {
          Table::new("lines", db)
              .with_column_of::<i64>("quantity")
              .with_one("product", "product", Product::table)  // a line can traverse out
      }, None)
  ```

  The closure builds the contained record's schema — same shape as `with_one`'s `build_target`,
  same type system, evaluated lazily. `vista_contained()` surfaces these as `ContainedSpec`s.

- **The sub-Vista** is [`vantage_vista::build_contained_vista`]. It materializes the column's
  records into an in-memory `ImTable`, serves reads from it, and on every write re-serializes the
  whole collection and calls a **writeback** closure. That writeback — patch the parent row's host
  column — is the only persistence-specific part you supply.

So your job is one `TableShell` method.

### `TableShell::get_contained_ref`

```rust
fn contained(&self) -> &IndexMap<String, ContainedSpec> {
    &self.metadata.contained
}

fn get_contained_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
    let rel = self.table.contained_relation(relation)?;          // host column, kind, build_target
    let host_value = /* the embedded collection as a CBOR map/array — see below */;
    let parent_id  = /* this row's id, as your native id */;

    // Columns for the sub-Vista's schema, harvested from the closure-built table.
    let columns = metadata_from_table(&rel.build_target(self.db())).columns;
    let spec    = ContainedSpec::new(rel.name(), rel.host_column(), rel.kind()).with_columns(columns);

    // Eager writeback: re-serialize → patch the host column on the parent row.
    let writeback = Arc::new(move |collection: CborValue| { /* patch parent[host] = collection */ });

    // Traverse-out: resolve the contained record's own relations (line → product).
    let ref_resolver = Arc::new(move |rel, child_row| { /* get_ref_from_row on the contained table */ });

    build_contained_vista(&spec, host_value.as_ref(), writeback, Some(ref_resolver))
}
```

And in the factory's `metadata_from_table`, copy the specs so the relation surfaces:

```rust
for spec in table.vista_contained() {
    metadata = metadata.with_contained(spec);
}
```

That's the whole integration. The two driver-specific decisions are how the host value crosses the
boundary, and what the writeback does — and those split cleanly along one line.

### Native vs JSON-blob

**Native backends** (SurrealDB, MongoDB) store the host column as a real nested object/array. The
value arrives as `CborValue::Map`/`Array` already, and the writeback patches it back as-is —
SurrealDB `UPDATE … MERGE`, MongoDB `$set`. No serialization on either side:

```rust
let host_value = row.get(rel.host_column()).cloned();       // already a Map/Array
// writeback: patch { host: AnyNativeType::from(collection) }
```

**JSON-blob backends** (SQL with no native nesting — the SQLite path) store the collection as a
JSON string in a `TEXT` column. Parse on read, serialize on write, using the shared
`json_to_cbor` / `cbor_to_json` bridge:

```rust
let host_value = row.get(rel.host_column()).and_then(parse_json_host);   // Text(json) → Map/Array
// writeback: patch { host: Text(cbor_to_json(collection).to_string()) }
```

`parse_json_host` also passes a `Map`/`Array` straight through, so the same code copes with a
backend that *does* parse JSON columns natively (Postgres `jsonb`, MySQL `json`) — there, declaring
the host column as `TEXT` keeps the round-trip a plain string and avoids the write-side bind for
nested values. Postgres and MySQL share the SQLite implementation verbatim for exactly this reason.

### Why not just flatten the keys?

You can, for *reading* a fixed shape — `inventory.stock` as a scalar column is fine when there's
one known field. It falls apart the moment the embedded data is a *collection* (an order has N
lines, not a fixed set), or the user needs to add and remove elements. A contained relation gives
you a record set with ids, not a bag of dotted columns.

### Why not model it as a foreign-key relation?

Because there's no other table to point at, and no foreign key to join on. The data is in the row.
Forcing it into `with_many` would mean inventing a synthetic table and writing a join that the
storage engine can't honour. Contained relations are the natural model: traversal is a column
projection, not a query.

### Eager writeback

Every mutation on the sub-Vista patches the parent row immediately — there's no `flush`. This keeps
the sub-Vista and the parent row coherent at all times, and it's deliberate: batching belongs to a
higher layer (a UI's write queue), not the storage boundary. The cost is one parent patch per edit,
which is trivial for the small collections this targets.

The flip side: the whole collection is re-serialized and written each time. A contained relation is
for line items and embedded objects, not for a thousand-element array you mutate in a tight loop.

### Sharp edges

**Declare the host column.** This is the bug every backend hits. If `lines` isn't a declared
column, your read path won't select or project it (SQL builds its `SELECT` from declared columns;
MongoDB projects from `column_paths`), so the parent row arrives without it and traversal sees an
empty collection. Declare it alongside the `with_contained_*` call.

**Positional ids shift.** With no declared id column, contained-many records are keyed by index
(`"0"`, `"1"`, …). Deleting element 0 renumbers the rest. Give the contained schema an id column
(`with_contained_many(…, Some("line_id"))`) when callers hold onto ids across mutations.

**Contained-one uses a fixed id.** A single embedded object is addressed as `"0"`; there's no
ambiguity to resolve.

**The writeback is not atomic with the contained record's own children.** A contained record can
traverse out (`line → product`) or even nest further, but each backend write is its own statement.
That's the same best-effort contract as nested insert (Step 6's neighbour) — fine for these shapes,
not a transaction.

### From YAML

Contained relations are declarable in a YAML vista spec too, via a `contained:` section that mirrors
`columns:`/`references:`:

```yaml
name: order
columns:
  id: { type: string, flags: [id] }
  lines: { type: string }          # the host column — declare it so it's selected
sqlite:
  table: order
contained:
  lines:
    host_column: lines
    kind: contains_many            # or contains_one
    id_column: line_id             # optional; omit for positional ids
    columns:
      product: { type: string }
      quantity: { type: int }
```

The loader lowers this through one generic helper —
[`Table::with_contained_specs`](https://docs.rs/vantage-table/latest/vantage_table/) — which calls
your driver's existing `build_column` on each contained column, so the YAML and code-first paths
converge on the same registration. Wiring it is one line per driver in `table_from_spec`:

```rust
table = table.with_contained_specs(&spec.contained, build_column)?;
```

**Limitation:** YAML-declared contained records carry *columns only* — no nested relations, so
traverse-out (`line.product`) isn't expressible from YAML yet (the code-first closure still supports
it, since it can add `with_one` to the contained table). Lifting this means letting the contained
columns carry `references:` sugar plus a resolver, the same machinery YAML foreign-key references
would need.

### Step 9 checklist

A backend supports contained relations once it has:

1. **`TableShell::contained()`** returning `&self.metadata.contained`.

2. **`TableShell::get_contained_ref`** — a thin shim that extracts the parent row's id (in the
   driver's native id type) and forwards to the shared
   [`Table::get_contained_ref`](https://docs.rs/vantage-table/latest/vantage_table/), passing three
   things only the driver knows: the `wrap` closure (target `Table` → `Vista` via its factory), and
   the host `decode`/`encode` codec (native passthrough, or JSON parse/serialize). The generic helper
   seeds the records, harvests the contained schema, wires the eager writeback, and resolves
   traverse-out.

3. **`metadata_from_table`** copying `table.vista_contained()` into `VistaMetadata`.

4. **YAML** — one line in `table_from_spec`:
   `table = table.with_contained_specs(&spec.contained, build_column)?;`

5. **Tests** (gated on `feature = "vista"`, against a real backend): declare a host column holding a
   collection (code-first *and* via `from_yaml`), traverse it, insert/patch through the sub-Vista, and
   re-read the parent row to prove the writeback landed.

Native and JSON-blob backends differ only in two closures — how the host value enters and how the
writeback leaves. Everything between is shared.
