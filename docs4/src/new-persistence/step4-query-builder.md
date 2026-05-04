# Step 3: Statement Builders and SelectableDataSource

In practice, nobody writes raw expressions for every query. This step adds the `Selectable` trait
implementation for your SELECT builder and wires it up through `SelectableDataSource` so the rest of
vantage can create and execute queries through a standard interface.

### Implement Selectable for your SELECT builder

The `Selectable<T>` trait is the standard interface for building SELECT queries across all vantage
backends. Your SELECT struct needs to implement it. The trait has two kinds of methods:

**Mutating methods** you must implement — `set_source`, `add_field`, `add_where_condition`,
`add_order_by`, `add_group_by`, `set_limit`, `set_distinct`, the `clear_*` methods, the `has_*`
methods, `as_count`, and `as_sum`.

**Builder methods** you get for free — `with_source`, `with_field`, `with_condition`, `with_order`,
`with_expression`, `with_limit`. These are default implementations that call the mutating methods
and return `self`.

This means your builder code is just the struct definition and `new()`:

```rust
pub struct SqliteSelect {
    pub fields: Vec<Expr>,
    pub from: Vec<Expr>,
    pub where_conditions: Vec<Expr>,
    pub order_by: Vec<(Expr, bool)>,
    pub group_by: Vec<Expr>,
    pub distinct: bool,
    pub limit: Option<i64>,
    pub skip: Option<i64>,
}

impl SqliteSelect {
    pub fn new() -> Self { /* initialize empty */ }
}
```

The `Selectable` impl goes in its own file (e.g., `statements/select/impls/selectable.rs`) and the
builder methods come from the trait:

```rust
let select = SqliteSelect::new()
    .with_source("product")
    .with_field("name")
    .with_field("price")
    .with_condition(sqlite_expr!("\"is_deleted\" = {}", false))
    .with_order(sqlite_expr!("\"price\""), false)
    .with_limit(Some(10), None);
```

The `as_count()` and `as_sum()` methods should clone the current query, replace the fields with
`COUNT(*)` or `SUM(column)`, drop the ORDER BY (unnecessary for aggregates), and render:

```rust
let count_expr = select.as_count();  // SELECT COUNT(*) FROM product WHERE ...
let sum_expr = select.as_sum(sqlite_expr!("\"price\""));  // SELECT SUM("price") FROM ...
```

### Implement SelectableDataSource

This trait connects the SELECT builder to execution. It tells vantage "this database can create
SELECT queries and run them":

```rust
impl SelectableDataSource<AnySqliteType> for SqliteDB {
    type Select = SqliteSelect;

    fn select(&self) -> Self::Select {
        SqliteSelect::new()
    }

    async fn execute_select(&self, select: &Self::Select) -> Result<Vec<AnySqliteType>> {
        // delegate to ExprDataSource::execute()
    }
}
```

### Live tests

Up to now, most tests used in-memory databases created in `setup()`. For Step 3, start running tests
against a real pre-populated database. This catches issues that in-memory tests miss — schema
mismatches, type affinity surprises, data edge cases.

Set up a test database with known data (we use a bakery schema translated from SurrealDB's test
fixture), and write tests that query it through the `Selectable` interface:

```rust
let db = SqliteDB::connect("sqlite:../target/bakery.sqlite?mode=ro").await?;

let select = SqliteSelect::new()
    .with_source("product")
    .with_condition(sqlite_expr!("\"price\" > {}", 200i64))
    .with_order(sqlite_expr!("\"price\""), false);

let record: Record<AnySqliteType> = db.associate(select.expr()).get().await?;
let product = Product::from_record(record)?;
assert_eq!(product.name, "Enchantment Under the Sea Pie");
```

### Implementing complex queries

The `Selectable` interface gives you the bare bones — fields, conditions, ordering, limits,
aggregates. Your database can do much more: JOINs, subqueries, CTEs, window functions, HAVING,
UNION, JSON operators, CASE expressions.

The best way to build this out is incrementally, driven by real queries:

1. **Create a test database scaffold** with enough tables and data to exercise complex features.
   Foreign keys, self-referential hierarchies, many-to-many junctions, JSON columns, generated
   columns — the more variety, the better. (See `scripts/sqlite/db/v3.sql` for an example.)

2. **Write the queries first** in raw SQL, as comments in your test file. Start with queries you
   know work against your scaffold. Each query should target specific features (JOINs, GROUP BY +
   HAVING, window functions, etc.).

3. **Implement one query at a time.** For each query:
   - Read the SQL and identify which builder methods are missing
   - Add the methods to your SELECT struct (e.g., `add_join`, `add_having`, `with_cte`)
   - Write a render test that checks the generated SQL matches
   - Write a live test that executes against the scaffold and verifies results

This approach has two advantages. First, you don't over-design — you only add features you actually
need. Second, every new feature ships with a test that proves it works against a real database, not
just string comparison.

When your queries outgrow what the select struct offers directly, extract **primitives** and
**nested structs** rather than bloating the builder. For example, `Identifier` (in
`vantage-sql/src/primitives/`) handles qualified column names (`"u"."name"`) and aliases — it
implements `Expressive<T>` so it plugs straight into expressions. Similarly, `SqliteSelectJoin`
lives inside the select module and renders its own `INNER JOIN ... ON ...` clause. The select struct
just holds a `Vec<SqliteSelectJoin>` and calls `render()` on each one. This pattern — small struct
with `Expressive` impl, composed into the builder — scales to CASE, CTE, window specs, and anything
else without the select struct growing unbounded.

### Other statements

`Selectable` only covers SELECT. INSERT, UPDATE, and DELETE don't need a trait at this stage — they
just need to implement `Expressive<AnySqliteType>` so they can be passed to
`ExprDataSource::execute()`. The statement builders from earlier steps still work, they just need
their expression type migrated from `JsonValue` to `AnySqliteType` to flow directly into
`execute()`.

### Step 3 conclusion

At this point you should have:

1. **`Selectable<AnyType>` impl** for your SELECT builder — all standard methods implemented,
   builder pattern provided by trait defaults.

2. **`SelectableDataSource<AnyType>` impl** for your DB struct — `select()` and `execute_select()`.

3. **Tests** in `tests/<backend>/3_*.rs` covering:
   - SQL rendering via `preview()` — fields, conditions, ordering, limits, distinct, group by
   - `as_count()` and `as_sum()` render correctly
   - Live execution against a test database — SELECT, COUNT, SUM, ORDER+LIMIT
   - Entity deserialization from live query results

