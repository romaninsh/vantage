# Implicit References

The previous chapter ([Expressions & Subqueries](./expressions.md)) closed with a recipe: take a
relation, call `get_subquery_as` to build a correlated subquery for one field, and register it with
`with_expression` so it projects as a column. That recipe works, but surfacing a related row's field
is common enough to deserve a declarative form.

That form is a **dotted name** in `Table::with_active_columns`. Write `"client.name"` and the table
traverses the declared `has_one` relation `"client"`, imports the target's `name` field, and
projects it as a read-only column aliased under the literal dotted name — no hand-written
expression. This feature is also called **implicit references**.

`with_active_columns` does two jobs at once:

1. **Plain names restrict projection.** Only the listed columns are selected — useful on its own for
   narrowing wide tables. The id column is always projected regardless.
2. **Dotted names traverse relations.** Each dot is one `has_one` hop from the current table to its
   target.

### One hop and two hops

Using the bakery model from this guide — Orders relate to Client via `client_order.client_id`, and
Client relates to Bakery via `client.bakery_id`, declared as relations `"client"` and `"bakery"`
with `with_one`:

```rust
// Plain names restrict projection; dotted names traverse has_one relations.
let orders = Order::sqlite_table(db).with_active_columns(&[
    "id",
    "client_id",
    "client.name",        // one hop:  client_order -> client
    "client.bakery.name", // two hops: client_order -> client -> bakery
])?;
```

`client.name` is one hop. `client.bakery.name` recurses through two — traversal supports an
arbitrary depth of `has_one` hops out of the box. On a SQL backend this lowers to nested correlated
scalar subqueries:

```sql
SELECT id,
  (SELECT name FROM client WHERE client.id = client_order.client_id) AS "client.name",
  (SELECT (SELECT name FROM bakery WHERE bakery.id = client.bakery_id)
     FROM client WHERE client.id = client_order.client_id)           AS "client.bakery.name"
FROM client_order
```

The alias is the dotted name itself, so rows come back with a **flat key equal to the dotted name**:

```rust
for row in orders.list_values().await?.values() {
    let get = |k: &str| row.get(k).map(|v| format!("{v}")).unwrap_or_default();
    println!(
        "  order {}: client.name={}, client.bakery.name={}",
        get("id"),
        get("client.name"),
        get("client.bakery.name"),
    );
}
```

There is no nested object to unwrap — `row.get("client.bakery.name")` is a plain lookup on a flat
record. The full example lives in `bakery_model3/examples/implicit-references.rs`; run it with
`cargo run -p bakery_model3 --example implicit-references`.

### Build-time validation

A design pillar of implicit references: every failure surfaces when the table is built, never as a
silently empty column at fetch time. Concretely:

- An unknown column or unknown relation in a dotted name → build error.
- A `has_many` hop → build error. Traversal is `has_one`-only, because a to-many field is a set, not
  a value. If you want an aggregate over the set — a count, a sum — that's the previous chapter's
  `get_count_query` territory.
- A backend that can lower neither a correlated subquery nor a native path (MongoDB, CSV, REST)
  refuses dotted names up front: its `TableSource::supports_traversal` is `false`. Traversal is also
  same-datasource only — enriching rows with fields from a *different* datasource belongs to Dio
  augmentation ([Dio](./dio.md)).

### Why not let a bad dotted name fail at fetch time?

Because a fetch-time failure in a projection is invisible. The query still runs, the column comes
back empty, and nothing tells you whether the relation was misspelled or the data is genuinely
absent. Building the table is the moment you have full knowledge — the relation registry, the
target's columns, the backend's capabilities — so that's where every check runs.

### Read-only semantics

An imported column is a computed projection. No backend can honestly store `client.name` on the
`client_order` table — the value lives in a different row of a different table. So imported columns
carry write-path enforcement:

- They are flagged `calculated` in vista metadata, so downstream consumers know the column is
  derived (see [Vistas](./vistas.md)).
- They are **stripped from full-record write payloads** — insert, replace, generated-id insert. A
  read-modify-save round-trip never persists `client.name` as a real field.
- They are **rejected outright in a `patch`**. A partial payload naming a read-only column is
  explicit intent, and silently dropping it would turn the patch into a successful no-op.

The key difference: strip vs reject. A full-record payload naturally contains everything you read —
including imported columns — so stripping them is the correct interpretation. A patch payload
contains only what you chose to send, so a read-only column there is a mistake worth an error.

Imported columns are also excluded from quicksearch and (on SurrealDB) not orderable — the dotted
name is a projection alias, not a physical field.

### Per-backend lowering

SQL backends lower dotted names through the generic chain: nested correlated scalar subqueries built
on the same `get_subquery_as`/`select_expression` machinery from the previous chapter. A backend
that already supports that recipe gets implicit references with nothing backend-specific to
implement.

SurrealDB overrides `TableSource::traversal_path_expr` and emits a native **idiom path** instead —
`client.name` — with each segment escaped separately. Escaping the joined path as one identifier
would produce a literal ⟨client.name⟩ field lookup: a dead column that matches nothing. The idiom
descends each hop's **link field**, not the relation's registry name — a relation declared
`with_one("owner", "client", …)` still lowers to `client.name`. Multi-hop comes for free in the
idiom.

### Why not just use with_expression?

You still can, and sometimes you should. The key difference: reach for `with_expression` +
`get_subquery_as` when you need an *arbitrary expression* — arithmetic, aggregates, raw escapes.
Reach for a dotted column when you just want a related field. The dotted form adds what the manual
recipe can't:

- **Build-time validation** — the manual recipe happily aliases a subquery over a misspelled column.
- **A typed imported column** — the definition is cloned from the target's column, so type metadata
  travels with it.
- **Enforced read-only write semantics** — a `with_expression` column has no write-path story at
  all.

### Gotchas

- `Table::derive_from` does not inherit implicit references. A derived table starts with no active
  set, and listing an imported dotted column in its column inheritance copies only a bare
  definition. Re-declare the dotted names on the derived table.
- Columns added after `with_active_columns` are not in the active set and won't project — declare
  the active set last.
- Expression-only columns (registered via `with_expression` with no column definition) may be named
  in the active set; they stay projected.
- Same-datasource only, by design — see [Dio](./dio.md) for the cross-datasource alternative.

### The YAML surface

In config-driven vistas (see [Config-Driven Vistas](../config-driven-vistas.md)), a dotted column
**name** in a table spec declares the same thing declaratively — the driver factory routes it
through the traversal import, with the same build-time validation:

```yaml
columns:
  - name: batch
    type: string
    references: batch
  - name: batch.name          # relation "batch" → column "name" on the target
    type: string
    optional: true
```

### Conclusion

At this point you should be able to:

1. **Restrict projection** with plain names in `with_active_columns` (the id column always
   projects).
2. **Import related fields** with dotted names — one hop or many, over declared `has_one` relations.
3. **Read them back** via flat keys equal to the dotted name.
4. **Predict every build-time error** — unknown column, unknown relation, `has_many` hop, backend
   without traversal support.
5. **Explain the write-path semantics** — stripped from full-record writes, rejected in a patch.
6. **Choose the right tool** — dotted column for a related field, `with_expression` for an arbitrary
   expression.
7. **Declare the same thing in YAML** for config-driven vistas.

The next page, [Vistas](./vistas.md), covers how the `calculated` flag and the
`can_traverse_in_columns` capability surface at the erased layer.
