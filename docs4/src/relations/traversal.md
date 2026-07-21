# Traversing Sets and Records

The [previous chapter](../relations.md) declared the bakery model: Bakery → Clients
(`client.bakery_id`) → Orders (`client_order.client_id`), with the relations `"orders"`,
`"client"`, and `"bakery"` registered via `with_one` / `with_many`. Declaring a relation stores
the join recipe. This chapter is about using it: turning "the orders of these clients" into an
actual query.

There are four traversal forms. They differ in *what you have in hand when you traverse* — a set
(conditions, no data), a loaded row, or a loaded record object — and, between the two set forms,
in *how the condition is applied* to the target. Each form exists because each starting point
lets the backend do something different.

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

In application code these calls hide behind the model's extension traits —
`paying.ref_orders()`, `orders.ref_client()` — so the string and the turbofish live once, next
to the table definition
([Coercion — pinning `get_ref`'s type](../relations.md#coercion--pinning-get_refs-type)).
This chapter spells out the underlying calls so you can see what each form generates.

How the `IN` condition is built is per-backend — each backend implements
`TableSource::related_in_condition`:

- **SQL** builds the subquery you see above: `"client_id" IN (SELECT "id" FROM "client" WHERE …)`.
- **MongoDB** has no subqueries, so it builds a deferred `{ field: { "$in": [...] } }` document —
  the parent ids are resolved when the child query runs.
- **CSV** fetches the join values in memory and builds an IN condition from them.

```admonish info title="MongoDB: the deferred condition"
MongoDB's `related_in_condition` returns a **deferred** condition — a closure, not a document.
Nothing runs when you traverse; the closure runs when the *child* query executes. At that point
it issues a projected `find` on the source collection — only the join column, with the source
set's own filter applied — collects the values, and emits `{ field: { "$in": [...] } }` for the
child query. It also absorbs a representation mismatch: ids may be stored as `ObjectId` on one
side and as the hex string on the other, so every collected value is pushed in *both* forms and
`$in` matches either. The set-to-set contract survives — you still hand the target one
condition — it just costs two round-trips at fetch time instead of one nested query.
```

```admonish info title="CSV: an in-memory IN list"
CSV has no query engine at all — its condition type is an ordinary Vantage expression that the
driver *evaluates per row in Rust* while reading the file. `related_in_condition` builds
`target_column.in_(values)`, where `values` is itself deferred: at fetch time it lists the source
table (with its conditions applied the same in-memory way), pulls the join column out of each
row, and yields the list as a single value. Filtering the target is then a per-row membership
check. Same declared relation, same traversal call — no database anywhere.
```

Use this form when the parent is a set — filtered, unfiltered, one row or a thousand.

## Form 2 — set-to-set, for embedding: `get_subquery_as`

Technically set-to-set as well — same starting point, same declared relation — but the condition
is applied differently. Where `get_ref_as` narrows the target with `IN (subquery)` so the related
rows can be *fetched* on their own, `Table::get_subquery_as::<E2>(relation)` attaches a
**correlated** condition — target column against source column, row by row:

```rust
let orders = clients.get_ref_as::<Order>("orders")?;      // fetch:  WHERE client_id IN (SELECT id FROM client …)
let orders = clients.get_subquery_as::<Order>("orders")?; // embed:  WHERE client_id = client.id
```

A correlated table is useless to fetch standalone — its condition references the source's rows —
but it is exactly right to *embed* as a scalar subquery inside the source's own SELECT: a
client's `order_count`, an order's `client.name`. The embedding recipe (`select_column`,
aggregates, composition) is the subject of the [next chapter](./expressions.md).

```admonish warning title="SQL and SurrealDB only"
This form needs the backend to express a correlated condition
(`TableSource::related_correlated_condition`), and only SQL backends and SurrealDB can. MongoDB,
CSV, REST, and CMD have no correlated-subquery expressions to lower to — the default
implementation panics rather than degrading silently. The fetching forms (1, 3, 4) remain
available everywhere.
```

## Form 3 — row-in-hand: `get_ref_from_row`

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

```admonish info title="Plumbing, not user-facing API"
You will rarely call this directly: it exists as the primitive beneath the blanket `get_ref`
implementations for `ActiveRecord` and `ActiveEntity` (form 4), and the erased `Vista::get_ref`
forwards here too. When a UI shows a clicked row's children, this is the form doing the work —
one equality filter — but the caller holds a record handle or a Vista, not a raw row.
```

## Form 4 — record-level: `GetRefExt::get_ref`

The same traversal as form 3, called from a loaded handle instead of a raw row:
`launch.get_ref::<LaunchCrew>("launch_crew")` works on an `ActiveEntity` (typed — its id is
injected into the row first, so has-many relations resolve) and on an `ActiveRecord` (untyped —
the raw row forwards directly). The method comes from the `GetRefExt` extension trait in
`vantage-table`, with blanket implementations for both handles over any `TableSource`. The
worked example (loading a record, traversing, inserting a child) lives in
[Records: Traversal, Invariants & Hooks](../record-lifecycle.md#traversing-from-a-loaded-record-get_ref).

## The bare target: `get_ref_target`

Sometimes you want the relation's target table with *no* condition at all —
`Table::get_ref_target::<E2>(relation)` builds exactly that. Where the traversal forms narrow
the target to related rows, `get_ref_target` hands you the table you'd insert a new related row
into before any join value exists (this is what Vista's nested insert uses).

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

## Conclusion

You can now:

1. **Traverse a filtered set** with `get_ref_as` — conditions compose into an `IN (subquery)`
   on the target.
2. **Correlate instead of fetch** with `get_subquery_as` — same relation, a per-row condition,
   built for embedding in the source's SELECT.
3. **Traverse from a loaded row or record** with `get_ref_from_row` / `get_ref` — one
   eq-condition, supported by every backend.
4. **Obtain a bare insert target** with `get_ref_target` — the relation's target with no
   condition.
5. **Declare embedded/contained relations** on document stores with `with_contained_many`,
   where traversal is descent rather than a query.
