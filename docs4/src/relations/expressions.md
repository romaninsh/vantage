# Expressions & Subqueries

The previous chapter used relations to *fetch* related rows — traverse from a client to their
orders, get the orders back as a set. This chapter uses relations *inside* a query: a client row
that carries its own order count, computed by the database, delivered as just another column.

The recipe has three parts:

1. **`Table::with_expression(name, |t| …)`** — adds a computed field to the table. It's evaluated
   as part of the SELECT, alongside the physical columns.
2. **`Table::get_subquery_as::<E2>(relation)`** — builds the relation's target table with a
   **correlated condition**. For the clients → orders relation that condition is
   `client_order.client_id = client.id` — the direction is always `target.<id> = source.<fk>`,
   supplied by the backend through `TableSource::related_correlated_condition`. This table is
   explicitly designed for embedding as a scalar subquery.
3. **`select_column("field")`** — projects a single column from that subquery, so it can sit where
   a scalar value is expected.

Here's the shape, straight from the `select_column` rustdoc:

```rust
.with_expression("category", |t| {
    t.get_subquery_as::<Category>("category").unwrap()
        .select_column("name")
        .expect("Category has a 'name' column")
})
```

The key difference from the previous chapter: `get_ref_as` narrows with `IN (subquery)` to *fetch*
related rows; `get_subquery_as` correlates to *embed* a related value per row. Same relation
declaration, two different query shapes.

`select_column` returns `Option<Expression<T::Value>>` — `None` when the field isn't a column on
the target table. When the column is hardcoded and known to exist, `.expect(...)` is the honest
choice: it panics in development, where you can fix the typo, rather than silently at runtime.

### Why not a JOIN?

Vantage composes *sets*. A correlated scalar subquery keeps the row shape of the source table —
one row per client, one extra column — with no join-key bookkeeping and no row multiplication
when a client has ten orders. It also nests arbitrarily, which joins don't do gracefully. And
you're not paying for the nesting: the database's optimizer flattens a correlated subquery where a
join would be equivalent.

### Aggregates over a relation

`get_subquery_as` returns a full `Table`, so the aggregate query-builders work on it:
`get_count_query()` and `get_sum_query(&col)`. Both wrap their output in parentheses, so they nest
safely inside the outer SELECT.

Here's the real thing — verbatim from `bakery_model3/src/client.rs`, a client table whose
`order_count` is computed by the database:

```rust
pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Client> {
    Table::new("client", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("email")
        .with_column_of::<String>("contact_details")
        .with_column_of::<bool>("is_paying_client")
        .with_one("bakery", "bakery", Bakery::surreal_table)
        .with_many("orders", "client", Order::surreal_table)
        .with_expression("order_count", |t| {
            let orders = t.get_subquery_as::<Order>("orders").unwrap();
            orders.get_count_query()
        })
}
```

The generated query has this shape (illustrative):

```sql
SELECT *, (SELECT COUNT(*) FROM order WHERE order.client = client.id) AS order_count
FROM client
```

The expression field shows up in `ReadableValueSet` results alongside the physical columns. To get
it onto your entity struct, add a matching `Option` field — it deserializes like any other column.

### Composing expressions

Expressions can reference other expressions. `get_column_expr(name)` resolves a name to either a
real column or a computed expression, so one expression can build on another — a `title` composed
of `name` and a count subquery via `concat_!`, for instance. All of it renders into a single
query. The nesting looks redundant on paper, but SQL engines optimize it away; there is no extra
round trip.

The Table chapter walks through a concrete title-composition example step by step — see
[Working with Tables](../intro/step2-tables.md) rather than repeating it here.

### Nesting arbitrary expressions — select_expression

`select_column` is sugar over a more general form. `Table::select_expression(expr)` wraps *any*
expression as a single-column subquery over the table's source and conditions —
`SELECT <expr> FROM table WHERE …` — clearing fields and ordering, since a scalar subquery has
exactly one output and no meaningful sort.

This is what lets one correlated subquery nest inside another: a two-hop lookup is
`outer.select_expression( (inner_subquery) )`. It is also the machinery under the next chapter's
implicit references, which automate exactly this recipe.

### Backend support

Correlated subqueries need backend support — the correlation condition comes from the backend's
`related_correlated_condition`, and not every backend can express one. The honest contract:

- **SQL backends and SurrealDB** implement it. This recipe works.
- **MongoDB, CSV, REST, CMD** leave the default, which panics. These backends have no correlated
  subqueries to lower to, so this is a SQL/SurrealDB technique — not a portable one.

```admonish info title="Cross-backend enrichment lives at the Dio layer"
If you need per-row related values across backends that can't correlate, there's a different tool
for that: augmentation, covered in [Relations and Dio](./dio.md).
```

### SurrealDB notes

SurrealDB's record links give it a second way to express a related field: a native idiom path.
`client.name` on an order traverses the link in place — no subquery at all, and often cheaper.
The [implicit references](./implicit-references.md) chapter uses exactly that lowering
automatically.

Correlated subqueries still work too. The `order_count` example above runs on SurrealDB as
written — `order.client = client.id` correlates through the record link.

### Conclusion

At this point you should be able to:

1. **Embed a related field** as a computed column — `with_expression` +
   `get_subquery_as::<E2>(relation)` + `select_column("field")`.
2. **Add relation aggregates** — `get_count_query()` and `get_sum_query(&col)` on the subquery
   table.
3. **Compose expressions** — `get_column_expr(name)` resolves columns and expressions alike, so
   expressions can build on each other inside one query.
4. **Nest subqueries** — `select_expression(expr)` wraps any expression as a scalar subquery, one
   inside another.
5. **Know where it runs** — SQL and SurrealDB correlate; MongoDB, CSV, REST, and CMD don't.

Next: [Implicit References](./implicit-references.md) — the declarative form of everything in this
chapter's first half.
