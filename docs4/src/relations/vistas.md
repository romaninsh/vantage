# Vista, YAML and Rhai

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

### The factory

A `Vista` is never constructed by hand — construction goes through a **vista factory** (the
`VistaFactory` trait, covered in
[Vista integration](../new-persistence/step8-vista-integration.md)). The factory's defining
ability is working **by name**: where typed code passes `User::table(db)`, a generic consumer
asks for `"users"` and gets a Vista back. How the name resolves is the factory's business —
typically it loads table specs from files — and that indirection is what lets config-driven
tools, scripts, and by-name traversal address models they have no Rust types for.

Behind a name, a vista is built one of two ways:

1. **From a typed table** — `from_table` wraps the table your model already builds:

   ```rust
   let vistas = vec![
       db.vista_factory().from_table(Client::surreal_table(db.clone()))?,
       sqlite.vista_factory().from_table(Product::sqlite_table(sqlite.clone()))?,
       csv.vista_factory().from_table(Order::csv_table(csv.clone()))?,
   ];
   ```

2. **From a YAML spec** — no Rust model at all: columns, relations, and computed fields declared
   as data ([Config-Driven Vistas](../config-driven-vistas.md)). This path is how the
   declarations in the last section of this page become live, traversable relations. A spec's
   *source* doesn't have to be a physical table, either — a `rhai:` block builds it from a query
   (a query-sourced vista), and a `base:` block derives it from another spec.

Whichever route constructs it, the factory folds the table's relations into `VistaMetadata` —
name, target, cardinality, foreign key. The erased handle carries enough to introspect the
relations and to traverse them. Nothing about the relation is lost in the erasure; only the
compile-time names are.

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

### Across datasources: the VistaCatalog

Everything above stays inside one datasource — a `Vista`'s references forward to the wrapped
table, and a single Vista deliberately knows nothing about *other* datasources. Cross-persistence
traversal lives one layer up, in the `VistaCatalog` (`vantage-vista-factory`): a name → Vista
catalog spanning many datasources, plus reference traversal between the models it holds.

You register models by name — each as a `ModelLoader` closure that builds a fresh, unconditioned
Vista, from whichever backend backs it — and then register relations *between* catalog models:

```rust
let mut cat = VistaCatalog::new();
cat.register("client", Arc::new(|| /* build the client Vista — one datasource */));
cat.register("bakery", Arc::new(|| /* build the bakery Vista — possibly another */));

cat.register_relation(
    "client",
    Relation::single_key("bakery", "bakery", ReferenceKind::HasOne, "id", "bakery_id"),
);

// From a loaded client row, traverse into the (possibly foreign) bakery model:
let bakery = cat.traverse(&cat.relations_for("client")[0], &client_row)?;
```

A `Relation` is a single-key (`target.foreign_key == parent_row[narrow_via]`) or multi-key join
description; `traverse` builds the target by name and pushes one eq-condition per key. How each
condition is honoured — SQL `WHERE`, in-memory filter, REST path/query param — is the target
driver's concern at fetch time.

`traverse_from` is the unified entry point: it prefers the parent Vista's own
**same-persistence** reference when the shell declares it and advertises
`can_traverse_to_record` (that path stays entirely inside one driver), and otherwise falls back
to a registered **cross-persistence** `Relation`. The catalog is also what the Dio layer's
augmentation uses to resolve its detail sources — the [next page](./dio.md) picks that up.

### Declaring relations in YAML and Rhai

Config-driven vistas (full chapter: [Config-driven vistas](../config-driven-vistas.md)) declare
relations in the table spec — either as column-level sugar or a top-level map. The factory reads
the spec, registers the relations, and the resulting `Vista` traverses them exactly like one built
`from_table`:

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

When a relation needs more than a plain foreign-key match — extra conditions, ordering, a search —
give the reference a `rhai:` build script:

```yaml
references:
  recent_orders:
    table: order
    kind: has_many
    foreign_key: client
    rhai: |
      table("order").add_condition_eq("client", row.id).add_order("created_at", "desc")
```

The script runs lazily when the relation is traversed, with the parent record in scope as `row`,
and must return a Vista — start it with `table("<name>")` and chain the conventional verbs
(`add_condition_eq`, `add_order`, `add_search`, `set_page_size`, `with_id`). Without `rhai:`, the
relation falls back to the plain `foreign_key` match. This is the `can_build_ref_via_script` path
from the capabilities list — the reference resolves through the script engine instead of the
fixed eq-condition.

Imported dotted columns — like `batch.name` above, the YAML form of
[implicit references](./implicit-references.md) — go through the same traversal import as the
Rust API, with the same construction-time validation: a bad dotted column fails the spec load,
not the first fetch. They arrive in metadata flagged `calculated`. That flag means read-only for
consumers: the value comes from a traversal, not from a column you can write. This is how a UI
knows not to offer editing on them.

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
4. **Traverse across datasources** — register models and `Relation`s on a `VistaCatalog`;
   `traverse_from` prefers the same-persistence reference and falls back to the catalog join.
5. **Declare relations in YAML and Rhai** — column-level `references:` sugar, the full top-level
   form, implicit dotted columns through a relation, and `rhai:` build scripts for traversals
   that need more than an FK match — all consumed by the same vista factory that wraps typed
   tables.
6. **Explain why `calculated` columns are read-only** in generic UIs — their values come from
   traversal, not from a writable column.

Next: [what happens above the Vista](./dio.md) — combining erased handles across backends.
