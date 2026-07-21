# Traversing Sets and Records

The [previous chapter](../relations.md) declared the bakery model: Bakery → Clients
(`client.bakery_id`) → Orders (`client_order.client_id`), with the relations `"orders"`,
`"client"`, and `"bakery"` registered via `with_one` / `with_many`. Declaring a relation stores
the join recipe. This chapter is about using it: turning "the orders of these clients" into an
actual query.

There are three traversal forms, and they differ in one question: *what do you have in hand
when you traverse?* A set (conditions, no data), a loaded row, or a loaded record object. Each
form exists because each starting point lets the backend do something different.

## Form 1 — set-to-set: `get_ref_as`

You have a *set* — a table narrowed by conditions, possibly matching many rows, none of them
loaded. `Table::get_ref_as::<E2>(relation)` traverses from the whole set at once:

```rust
let mut paying = Client::sqlite_table(db.clone());
paying.add_condition(paying["is_paying_client"].eq(true));

let orders = paying.get_ref_as::<Order>("orders")?;
// SELECT ... FROM "client_order"
// WHERE "client_id" IN (SELECT "id" FROM "client" WHERE "is_paying_client" = 1)
```

No client rows are fetched. The traversal generates an `IN (subquery)` condition on the target,
and the subquery is whatever conditions the source set carries. Narrow `paying` further and the
subquery narrows with it — the traversal call never changes. That composability is the point:
you build the set that describes *which* parents you mean, and the child set inherits that
description.

How the `IN` condition is built is per-backend — each backend implements
`TableSource::related_in_condition`:

- **SQL** builds the subquery you see above: `"client_id" IN (SELECT "id" FROM "client" WHERE …)`.
- **MongoDB** has no subqueries, so it builds a deferred `{ field: { "$in": [...] } }` document —
  the parent ids are resolved when the child query runs.
- **CSV** fetches the join values in memory and builds an IN condition from them.

Use this form when the parent is a set — filtered, unfiltered, one row or a thousand.

## Form 2 — row-in-hand: `get_ref_from_row`

You already hold a loaded row — a `Record<T::Value>` that came back from the database. There is
nothing to compute: the join value is sitting in the row. `Table::get_ref_from_row::<E2>(relation,
&row)` reads it out and applies it as a single eq-condition on the target:

```rust
let orders = clients.get_ref_from_row::<Order>("orders", &row)?;
// WHERE "client_id" = <the id read out of row> — one eq-condition
```

Which field gets read depends on the relation's direction: a `HasOne` relation reads its stored
foreign-key column from the row; a `HasMany` relation reads the source's id field. No subquery,
no deferred fetch — `row` already carries the value.

This is the form UIs use when the user clicked a row: the row is loaded, the click means "show
me its children", and the traversal costs one equality filter.

## Form 3 — record-level: `GetRefExt::get_ref`

The record-level equivalent of form 2, for when you hold a loaded `ActiveEntity` (typed) or
`ActiveRecord` (untyped) rather than a raw row:

```rust
use vantage_table::prelude::GetRefExt;

let launch = launches.get_entity(id).await?.expect("launch");

// A child set scoped to this launch — and carrying its foreign key (see invariants below).
let crew = launch.get_ref::<LaunchCrew>("launch_crew")?;
crew.insert_return_id(&LaunchCrew { astronaut_id: Some(a), role: Some("Pilot".into()), ..Default::default() }).await?;
```

`get_ref::<E2>(relation)` returns a `Table<T, E2>` scoped to the parent. Under the hood it is
the row-in-hand traversal: for a typed `ActiveEntity` the entity's id is injected into the row
before traversal (so has-many relations resolve); an untyped `ActiveRecord` already holds the
raw row and forwards directly.

The key difference between the forms: set-to-set composes conditions into a subquery and needs a
backend that can express one; row-in-hand and record-level need only equality filtering, which
every backend has. The capability contract makes this explicit — from the `VistaCapabilities`
docs:

- `can_traverse_to_record` — "Record-level reference traversal via `get_ref(relation, row)` —
  read the join value out of a known row and narrow the target with a plain eq-condition. Every
  backend that can filter by equality supports this (SQL, CSV, Mongo, Surreal, REST/GraphQL)."
- `can_traverse_to_set` — "Set-level reference traversal — narrow the target with an
  `IN (subquery)` derived from the parent's own conditions (the `get_ref_as` / reports path).
  Requires the backend to support subqueries; SQL and SurrealDB do, CSV/Mongo/REST do not."

Note that CSV and Mongo still serve `get_ref_as` at the Table level — they materialize the IN
list, as described above. The capability flag governs the erased Vista layer, covered in
[Relations on Vistas](./vistas.md).

## Inserting through a traversed set

A traversed set is a set like any other, and rows written into it must conform to it. Traversal
registers the foreign key as a **set invariant**: the `crew` set above doesn't just *filter* by
`launch_id` — it *asserts* it. An inserted child needs no FK (it is filled in), may state the
matching one (kept), and cannot smuggle a different one (the write errors):

| record's value for the column | result |
| --- | --- |
| absent | set to the invariant value |
| present but null | set to the invariant value |
| present and equal | kept |
| present and **conflicting** | the write is rejected with an error |

This is why the `LaunchCrew` insert in form 3 sets no `launch_id` — the invariant fills it. The
full treatment of invariants (hooks, `with_invariant`, `InvariantValue`) is in
[Record Lifecycle](../record-lifecycle.md).

## The bare target: `get_ref_target`

Sometimes you want the relation's target table with *no* condition at all —
`Table::get_ref_target::<E2>(relation)` builds exactly that. Where the three traversal forms
narrow the target to related rows, `get_ref_target` hands you the table you'd insert a new
related row into before any join value exists (this is what Vista's nested insert uses).

## Contained relations

Some backends embed related data *inside* the row instead of linking to another table.
SurrealDB's `Order` carries an embedded `array<object>` of order lines — declared as a column
(so it is selected) *and* as a contains-many relation whose record schema is built by a closure.
From `bakery_model3/src/order.rs`:

```rust
pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Order> {
    Table::new("order", db)
        .with_id_column("id")
        .with_column_of::<Thing>("client")
        .with_column_of::<bool>("is_deleted")
        // `lines` is an embedded `array<object>` of `{ product, quantity,
        // price }` — declared as a column so it's selected, and as a
        // contains-many relation whose record schema is built by the
        // closure (like `with_many`).
        .with_column_of::<AnySurrealType>("lines")
        .with_one("client", "client", Client::surreal_table)
        .with_contained_many(
            "lines",
            "lines",
            |db| {
                Table::new("lines", db)
                    .with_column_of::<Thing>("product")
                    .with_column_of::<i64>("quantity")
                    .with_column_of::<i64>("price")
                    // a line traverses out to the real product table
                    .with_one("product", "product", Product::surreal_table)
            },
            None,
        )
}
```

Contained relations are a K/V-and-document-store shape: the value already holds the related
data, so "traversal" is descent into the row, not a query. Note that a contained record can
still hold references back out — a line's `product` field is a `with_one` to the real product
table, so from an embedded line you traverse out with the same forms as anywhere else.

```admonish note title="SurrealDB record ids in narrowings"
Join values in SurrealDB are record ids (`Thing`), not scalar foreign keys. A string of the
shape `"table:key"` handed into a narrowing — from a script surface or JSON, say — is coerced
back into a record id by the backend (`coerce_reference_value` on `TableSource`; the identity
function on scalar-FK backends). String ids narrow correctly; they don't silently match nothing.
```

There is a fourth form, `get_subquery_as`, which exists purely for embedding a related query
inside a SELECT expression — that's the subject of the
[next chapter](./expressions.md).

## Conclusion

You can now:

1. **Traverse a filtered set** with `get_ref_as` — conditions compose into an `IN (subquery)`
   on the target.
2. **Traverse from a loaded row or record** with `get_ref_from_row` / `get_ref` — one
   eq-condition, supported by every backend.
3. **Insert children without writing foreign keys** — traversal registers the FK as a set
   invariant, and conflicting values are rejected.
4. **Obtain a bare insert target** with `get_ref_target` — the relation's target with no
   condition.
5. **Declare embedded/contained relations** on document stores with `with_contained_many`,
   where traversal is descent rather than a query.
