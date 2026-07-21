# Relations on Vistas

Everything so far assumed you could write `Table<SqliteDB, Order>` in your source code. Generic
consumers can't. A UI grid, an admin panel, a scripting surface, a config-driven tool — none of
them know your model types at compile time. What they hold is a `Vista`: the schema-bearing
runtime handle introduced in [the Vista chapter](../intro/step4-vista.md).

The question this page answers: what happens to relations when the type is erased? The answer is
that they survive in three forms:

1. **Metadata** — the relation names, targets, and cardinalities are introspectable, so a
   consumer can discover them at runtime.
2. **Traversal** — `get_ref` still works. Hand it a loaded row, get back a new `Vista` narrowed
   to the related rows.
3. **Capabilities** — an honest contract about which traversal forms this backend actually
   serves, so a consumer knows what to offer before trying.

### Getting a Vista

A `Vista` comes from the backend's vista factory (covered in
[Vista integration](../new-persistence/step8-vista-integration.md)):

```rust
let vistas = vec![
    db.vista_factory().from_table(Client::surreal_table(db.clone()))?,
    sqlite.vista_factory().from_table(Product::sqlite_table(sqlite.clone()))?,
    csv.vista_factory().from_table(Order::csv_table(csv.clone()))?,
];
```

The part that matters for this page: `from_table` folds the table's relations into
`VistaMetadata` — name, target, cardinality, foreign key. The erased handle carries enough to
introspect the relations and to traverse them. Nothing about the relation is lost in the erasure;
only the compile-time names are.

### Metadata: introspecting relations

`Vista` exposes the relation metadata directly:

- `get_references() -> Vec<String>` — the relation names.
- `list_references() -> Vec<(String, ReferenceKind)>` — names paired with cardinality
  (`ReferenceKind::HasOne` / `HasMany`).
- `get_reference(name) -> Option<&Reference>` — the full reference entry.
- `list_contained()` — contained (embedded) relations with their kind.

This is what lets a UI render relation tabs or context-menu entries without knowing the model.
`list_references` tells it "orders (has_many), country (has_one)" — enough to label a tab and
decide whether the target is a single row or a set — and it learned that from the handle, not
from your source code.

### Traversal: get_ref on a loaded row

The erased twin of the typed `get_ref_from_row`:

```rust
pub fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista>
```

Hand it a loaded row — `Record<CborValue>` is the erased record type — and get back a new `Vista`
narrowed to the related rows. The relation is named by string because that's all a generic
consumer has; the metadata folded in by the factory supplies the target and foreign key.

There is also `get_ref_target(relation) -> Result<Vista>` — the bare, unconditioned target. This
is the insert destination for nested creates: when a UI wants to add a related row, it needs the
target table without any narrowing applied.

Both return `Vista`, so traversal chains and the consumer never leaves the erased world. A grid
that drills from clients into orders into line items is calling `get_ref` three times, each time
holding nothing more specific than a `Vista` and a row.

```admonish info title="Record ids that round-tripped through JSON"
At the erased layer, SurrealDB record ids travel as strings (`"table:key"`). The backend coerces
a string-shaped record id back into a typed record id when narrowing, so a row that round-tripped
through JSON or a script still traverses correctly.
```

### The scripting surface

The same traversal is exposed to Rhai data scripts (the `vantage-vista` rhai feature). A script
holding a row can hop a relation:

```rhai
let t = table("tag");
let row = t.get_some();
let course = t.get_ref("golf_course", row);
```

There's no separate scripting implementation here — the erased `Vista` is what the script engine
wraps, and `get_ref` there follows the same metadata.

### Capabilities: the honest contract

Not every backend serves every traversal form — the [traversal chapter](./traversal.md) already
showed that set-level traversal needs subqueries. At the typed layer, you knew that when you wrote
the code. At the erased layer, the consumer has to ask. That's what `VistaCapabilities` is for.
The flags relevant to relations:

- `can_traverse_to_record` — "Record-level reference traversal via `get_ref(relation, row)` —
  read the join value out of a known row and narrow the target with a plain eq-condition. Every
  backend that can filter by equality supports this (SQL, CSV, Mongo, Surreal, REST/GraphQL)."
- `can_traverse_to_set` — "Set-level reference traversal — narrow the target with an
  `IN (subquery)` derived from the parent's own conditions (the `get_ref_as` / reports path).
  Requires the backend to support subqueries; SQL and SurrealDB do, CSV/Mongo/REST do not."
- `can_build_ref_via_script` — "Per-reference Rhai-scripted traversal — a reference carrying a
  `build_script` resolves through the script engine … rather than the fixed FK eq-condition
  path."
- `can_traverse_in_columns` — whether the backend can lower a dotted active column
  (`country.name`) into its own query; `true` for SQL and SurrealDB shells, `false` for
  CSV/Mongo/REST.

Consumers branch on these before offering the corresponding affordance: a report builder checks
`can_traverse_to_set` before offering set-level aggregation; a grid checks
`can_traverse_in_columns` before letting the user add a dotted column. The flags are also
readable by name (`capability_flag("can_traverse_to_set")`) and appear in the rhai capabilities
map, so scripts and config-driven tools can branch the same way.

### Declaring relations in YAML

Config-driven vistas (full chapter: [Config-driven vistas](../config-driven-vistas.md)) declare
relations in the table spec — either as column-level sugar or a top-level map:

```yaml
columns:
  - name: batch
    type: string
    references: batch        # sugar: has_one to table "batch", FK = this column
  - name: batch.name         # implicit reference through that relation
    type: string
    optional: true
references:
  tags:
    table: tag
    kind: has_many
    foreign_key: batch
```

The column-level `references: batch` sugar names the relation after the *target table*, which is
the common case. The full form (`references: { table, kind, foreign_key, name }`) can name it
differently — you need that when two relations point at the same table.

A reference may also carry a `rhai:` build script for traversals that need more than an FK match
(conditions, ordering). That's the `can_build_ref_via_script` path from the capabilities list —
the reference resolves through the script engine instead of the fixed eq-condition.

Imported dotted columns — like `batch.name` above, the YAML form of
[implicit references](./implicit-references.md) — arrive in metadata flagged `calculated`. That
flag means read-only for consumers: the value comes from a traversal, not from a column you can
write. This is how a UI knows not to offer editing on them.

The key difference: at the typed layer *you* know which traversal forms are safe — you wrote the
code against a backend you chose. At the erased layer, the *capabilities* say so. Same relations,
discovered instead of known.

### Conclusion

At this point you should be able to:

1. **Enumerate a Vista's relations** — `get_references`, `list_references` with cardinality,
   `get_reference` for the full entry, `list_contained` for embedded relations.
2. **Traverse from a loaded erased row** — `get_ref(relation, row)` returns a narrowed `Vista`;
   `get_ref_target(relation)` returns the bare target for nested creates.
3. **Branch on the four traversal capabilities** — `can_traverse_to_record`,
   `can_traverse_to_set`, `can_build_ref_via_script`, `can_traverse_in_columns` — before
   offering an affordance.
4. **Declare relations in YAML** — column-level `references:` sugar, the full top-level form,
   and implicit dotted columns through a relation.
5. **Explain why `calculated` columns are read-only** in generic UIs — their values come from
   traversal, not from a writable column.

Next: [what happens above the Vista](./dio.md) — combining erased handles across backends.
