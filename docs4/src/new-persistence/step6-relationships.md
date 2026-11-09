# Step 6: Relationships

Tables can declare relationships using `with_one` and `with_many`, then traverse them at runtime
with `get_ref_as`. The relationship system is provided by `vantage-table` — your backend just needs
`column_table_values_expr` implemented to make it work.

```admonish tip
This step covers what a *driver* must implement for relationships. For the *consumer* side —
traversing from a loaded record with `get_ref`, and the foreign-key invariants traversal sets up —
see [Records: Traversal, Invariants & Hooks](../record-lifecycle.md).
```

Implement `column_table_values_expr` — it builds a subquery for a single column respecting current
conditions. For SQL backends this is a `SELECT "col" FROM "table" WHERE ...` expression.

Define relationships when constructing tables — `with_one` for foreign-key-to-parent, `with_many`
for parent-to-children. Then traverse:

```rust,no_run
# use vantage_dataset::ReadableDataSet;
# use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
# use vantage_sql::sqlite_expr;
# use vantage_table::table::Table;
# use vantage_types::{EmptyEntity, entity};
# #[entity(SqliteType)]
# #[derive(Debug, Clone)]
# struct ClientOrder {
#     client_id: String,
# }
# fn client_order_table(db: SqliteDB) -> Table<SqliteDB, ClientOrder> {
#     Table::new("client_order", db)
#         .with_id_column("id")
#         .with_column_of::<String>("client_id")
# }
# fn client_table(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
#     Table::new("client", db)
#         .with_id_column("id")
#         .with_column_of::<bool>("is_paying_client")
#         .with_many("orders", "client_id", client_order_table)
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let db = SqliteDB::connect("sqlite:../target/bakery.sqlite?mode=ro").await?;
let mut clients = client_table(db);
clients.add_condition(sqlite_expr!("{} = {}", (clients["is_paying_client"]), true));

let orders = clients.get_ref_as::<ClientOrder>("orders").unwrap();

// The generated query includes the subquery:
// SELECT ... FROM "client_order"
//   WHERE client_id IN (SELECT "id" FROM "client" WHERE is_paying_client = 1)
assert_eq!(orders.list().await.unwrap().len(), 3);
# Ok(())
# }
```


`with_expression` adds computed fields to a table using correlated subqueries. It pairs with
`get_subquery_as` which produces `target.fk = source.id` conditions (vs `get_ref_as` which uses
`IN (subquery)`).

```text
.with_many("orders", "client_id", Order::sqlite_table)
.with_expression("order_count", |t| {
    t.get_subquery_as::<Order>("orders").unwrap().get_count_query()
})
// Generates: (SELECT COUNT(*) FROM "client_order"
//   WHERE "client_order"."client_id" = "client"."id") AS "order_count"
```

**What to implement:** override `related_correlated_condition` in your `TableSource` to produce
table-qualified equality, and `try_related_correlated_condition` to return it. `get_subquery_as`
calls the `try_` one. Its default calls `related_correlated_condition` when `supports_traversal`
is `true` and otherwise returns an `Unsupported` error, so on a backend without correlated
subqueries (CSV, REST) `get_subquery_as` fails when the table is built, not when it's read.
MongoDB traverses with `$lookup`, so it overrides the `try_` one to refuse.

```text
fn related_correlated_condition(&self, target_table: &str, target_field: &str,
    source_table: &str, source_column: &str) -> Self::Condition {
    sqlite_expr!("{} = {}", (ident(target_field).dot_of(target_table)),
        (ident(source_column).dot_of(source_table))).into()
}

fn try_related_correlated_condition(&self, target_table: &str, target_field: &str,
    source_table: &str, source_column: &str) -> Result<Self::Condition> {
    Ok(self.related_correlated_condition(target_table, target_field, source_table, source_column))
}
```

### Why two methods?

`related_correlated_condition` predates roll-ups and returns the condition itself, with a panicking
default. Changing it to return `Result` would break every driver that overrides it. The fallible
sibling keeps those drivers compiling: one that overrides only the old method still builds, and
if it says `supports_traversal`, its `get_subquery_as` keeps working through the `try_` default.

Requires `SelectableDataSource` (Step 3) since aggregate query builders use `table.select()`.

## Roll-up columns

An order's total over its lines, a client's order count, the date of the last payment: a value
computed per parent row from its children is the most common computed column. `with_rollup`
declares one over a `has_many` relation:

```rust
# use vantage_dataset::traits::ReadableValueSet;
# use vantage_sql::sqlite::{SqliteDB, sqlite_ident};
# use vantage_sql::sqlite_expr;
# use vantage_table::table::{RollupOp, Table};
# use vantage_types::EmptyEntity;
# fn line_table(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
#     Table::new("order_line", db)
#         .with_id_column("id")
#         .with_column_of::<String>("order_id")
#         .with_column_of::<i64>("price")
# }
# fn order_table(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
#     Table::new("client_order", db)
#         .with_id_column("id")
#         .with_many("lines", "order_id", line_table)
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let db = SqliteDB::connect("sqlite::memory:").await?;
# for sql in [
#     "CREATE TABLE client_order (id TEXT PRIMARY KEY)",
#     "CREATE TABLE order_line (id INTEGER PRIMARY KEY, order_id TEXT, price INTEGER)",
#     "INSERT INTO client_order VALUES ('order1'), ('order2')",
#     "INSERT INTO order_line (order_id, price) VALUES ('order1', 120), ('order1', 135), ('order1', 199)",
# ] {
#     sqlx::query(sql).execute(db.pool()).await?;
# }
let over_150 = sqlite_expr!("{} > {}", (sqlite_ident("price")), 150i64);
let orders = order_table(db)
    .with_rollup("line_count", "lines", RollupOp::Count, None, None)?
    .with_rollup("total", "lines", RollupOp::Sum, Some("price"), None)?
    .with_rollup("dearest", "lines", RollupOp::Max, Some("price"), None)?
    .with_rollup("big_lines", "lines", RollupOp::Count, None, Some(over_150.into()))?;

let rows = orders.list_values().await?;
let get = |id: &str, column: &str| serde_json::Value::from(rows[id][column].clone());
assert_eq!(get("order1", "total"), 454);
assert_eq!(get("order1", "big_lines"), 1);
// order2 has no lines
assert_eq!(get("order2", "total"), 0);
assert_eq!(get("order2", "dearest"), serde_json::Value::Null);
# Ok(())
# }
```

On SQL each roll-up is one correlated subquery in the SELECT:

```text
SELECT "id",
  (SELECT COUNT(*) FROM "order_line"
    WHERE "order_line"."order_id" = "client_order"."id") AS "line_count",
  (COALESCE((SELECT SUM(price) FROM "order_line"
    WHERE "order_line"."order_id" = "client_order"."id"), 0)) AS "total",
  ...
FROM "client_order"
```

The fourth argument is the child column; every op but `Count` needs one. The fifth is an optional
condition on the child rows, in the backend's own condition type. The column is read-only, like any
`with_expression` column.

| Op      | Computes             | Over no rows |
| ------- | -------------------- | ------------ |
| `Count` | number of child rows | `0`          |
| `Sum`   | sum of `column`      | `0`          |
| `Min`   | smallest `column`    | null         |
| `Max`   | largest `column`     | null         |
| `Avg`   | mean of `column`     | null         |

A missing column, an unknown column or relation, or a backend without roll-ups is an error from
`with_rollup` itself. A table that builds will read.

### Why not `with_expression` and `get_subquery_as`?

On SQL that's exactly what `with_rollup` builds, plus the `COALESCE` that makes an empty sum read
`0` instead of null. The difference is who writes the query. A hand-written `with_expression` bakes
in one backend's form. `with_rollup` asks the backend through `TableSource::rollup_expression`, so
SurrealDB can render its record-reference form and MongoDB can refuse with a clear error, from the
same call.

### How each backend renders it

| Backend                  | Renders                                                                         | Supported                    |
| ------------------------ | ------------------------------------------------------------------------------- | ---------------------------- |
| SQLite, Postgres, MySQL  | correlated subquery (the default)                                               | yes                          |
| SurrealDB                | `math::sum(<~(line FIELD invoice).net)` over a `REFERENCE` link, else `$parent` | yes                          |
| MongoDB                  | —                                                                               | not yet: `Unsupported` error |
| CSV, REST, other drivers | —                                                                               | `Unsupported` error          |

On SurrealDB, `<~` sees only links declared `REFERENCE`. `SurrealDB::check_references` reads the
schema once, after connecting; until it runs, and over a link without `REFERENCE`, a roll-up uses
the correlated `$parent` form and logs a warning with the `DEFINE FIELD … REFERENCE;` line that
would make it fast.

### What to implement

Nothing, if your backend has `try_related_correlated_condition` and a `Selectable` with `as_count`,
`as_sum`, `as_min`, `as_max` and `as_avg`: the default `rollup_expression` composes them. Override
`rollup_expression` to render something better than a correlated subquery, or to refuse:

```text
fn rollup_expression<E>(&self, parent: &Table<Self, E>, relation: &str, op: RollupOp,
    column: Option<&str>, condition: Option<Self::Condition>) -> Result<Expression<Self::Value>>
where E: Entity<Self::Value> + 'static {
    Err(error!("rollup columns aren't supported on MongoDB yet").mark_unsupported())
}
```

`as_avg` has a default that renders SQL's `AVG(column)`; a backend with another spelling
overrides it, as SurrealDB does with `math::mean`.

Checklist:

- `related_correlated_condition` returns table-qualified equality and
  `try_related_correlated_condition` wraps it in `Ok`, or both keep their defaults.
- `Selectable::as_avg` renders your backend's mean, if it isn't `AVG`.
- `rollup_expression` is overridden only to render a better form or to refuse.
- An empty child set reads `0` for `Count` and `Sum`, and null for the rest.

