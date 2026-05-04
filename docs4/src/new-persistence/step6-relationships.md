# Step 5: Relationships

Tables can declare relationships using `with_one` and `with_many`, then traverse them at runtime
with `get_ref_as`. The relationship system is provided by `vantage-table` — your backend just needs
`column_table_values_expr` implemented to make it work.

Implement `column_table_values_expr` — it builds a subquery for a single column respecting current
conditions. For SQL backends this is a `SELECT "col" FROM "table" WHERE ...` expression.

Define relationships when constructing tables — `with_one` for foreign-key-to-parent, `with_many`
for parent-to-children. Then traverse:

```rust
let mut clients = client_table(db);
clients.add_condition(sqlite_expr!("{} = {}", (clients["is_paying_client"]), true));

let orders = clients.get_ref_as::<SqliteDB, ClientOrder>("orders").unwrap();

// The generated query includes the subquery:
// SELECT ... FROM "client_order"
//   WHERE client_id IN (SELECT "id" FROM "client" WHERE is_paying_client = 1)
assert_eq!(orders.list().await.unwrap().len(), 3);
```


`with_expression` adds computed fields to a table using correlated subqueries. It pairs with
`get_subquery_as` which produces `target.fk = source.id` conditions (vs `get_ref_as` which uses
`IN (subquery)`).

```rust
.with_many("orders", "client_id", Order::sqlite_table)
.with_expression("order_count", |t| {
    t.get_subquery_as::<Order>("orders").unwrap().get_count_query()
})
// Generates: (SELECT COUNT(*) FROM "client_order"
//   WHERE "client_order"."client_id" = "client"."id") AS "order_count"
```

**What to implement:** override `related_correlated_condition` in your `TableSource` to produce
table-qualified equality. Default panics — backends without correlated subquery support (CSV) simply
can't use this feature.

```rust
fn related_correlated_condition(&self, target_table: &str, target_field: &str,
    source_table: &str, source_column: &str) -> Self::Condition {
    sqlite_expr!("{} = {}", (ident(target_field).dot_of(target_table)),
        (ident(source_column).dot_of(source_table)))
}
```

Requires `SelectableDataSource` (Step 3) since aggregate query builders use `table.select()`.

