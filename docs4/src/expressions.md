# Expressions & Queries

Vantage builds queries without string concatenation. The `vantage-expressions` crate provides a
type-safe, composable expression system that works across all persistence backends — SQL, SurrealDB,
MongoDB, CSV, and anything you add yourself.

<!-- toc -->

---

## The core idea

An `Expression<T>` is a template string with typed parameters:

```rust
# use vantage_sql::primitives::identifier::ident;
# use vantage_sql::sqlite_expr;
let expr = sqlite_expr!("SELECT {} FROM {} WHERE {} > {}",
    (ident("name")), (ident("product")), (ident("price")), 100i64);
// → SELECT "name" FROM "product" WHERE "price" > ?1  (with 100 bound as i64)
# assert_eq!(expr.preview(), r#"SELECT "name" FROM "product" WHERE "price" > 100"#);
```

Parameters are never interpolated into the string. They're carried separately, each tagged with a
type marker from your persistence's type system. The bind layer uses these markers to call the right
driver method — `bind_i64`, `bind_str`, `bind_bool` — no guessing, no silent coercion. (`preview()`
renders the parameters inline for reading; it is not what is sent.)

Three kinds of parameters:

- **Scalar** — a typed value: `42i64`, `"hello"`, `true`
- **Nested** — another expression, composed into the template
- **Deferred** — a closure that executes later (cross-database resolution)

---

## Vendor macros

Each persistence provides a convenience macro that produces `Expression<AnyType>` with the correct
type wrapping:

```rust
# use vantage_sql::{mysql_expr, postgres_expr, sqlite_expr};
# use vantage_surrealdb::surreal_expr;
let e = sqlite_expr!("SELECT * FROM product WHERE price > {}", 100i64);
let e = surreal_expr!("SELECT * FROM product WHERE price > {}", 100i64);
let e = postgres_expr!("SELECT * FROM product WHERE price > {}", 100i64);
let e = mysql_expr!("SELECT * FROM product WHERE price > {}", 100i64);
# let _ = e;
```

Same syntax, different type universes. The compiler ensures you can't accidentally mix a
`Expression<AnySqliteType>` into a SurrealDB query.

---

## Composing expressions

Expressions nest naturally. Parenthesised arguments call `.expr()` automatically:

```rust
# use vantage_sql::primitives::identifier::ident;
# use vantage_sql::sqlite_expr;
let condition = sqlite_expr!("{} > {}", (ident("price")), 100i64);
let query = sqlite_expr!("SELECT {} FROM {} WHERE {}",
    (ident("name")), (ident("product")), (condition));
# assert_eq!(query.preview(), r#"SELECT "name" FROM "product" WHERE "price" > 100"#);
```

The `ExpressionFlattener` collapses all nesting into a single flat template with positional
parameters — each one still carrying its type marker.

For building lists (e.g. multi-row INSERT), use `Expression::from_vec`:

```rust
# use vantage_expressions::Expression;
# use vantage_sql::sqlite_expr;
let row1 = sqlite_expr!("({}, {})", "tart", 220i64);
let row2 = sqlite_expr!("({}, {})", "pie", 299i64);
let rows = Expression::from_vec(vec![row1, row2], ", ");
# let _ = rows;
```

---

## Identifier quoting

SQL identifiers need quoting — and each database uses different quote characters. The `Identifier`
struct handles this by implementing `Expressive<T>` for each backend type:

```rust
# use vantage_sql::primitives::identifier::ident;
# use vantage_sql::{mysql_expr, sqlite_expr};
// Quoting adapts to the expression's type context
sqlite_expr!("SELECT {}", (ident("name")));      // → SELECT "name"
mysql_expr!("SELECT {}", (ident("name")));        // → SELECT `name`

// Qualified identifiers
sqlite_expr!("SELECT {}", (ident("name").dot_of("u")));  // → SELECT "u"."name"

// Aliases
mysql_expr!("SELECT {}", (ident("name").with_alias("n"))); // → SELECT `name` AS `n`
```

---

## ExprDataSource — executing expressions

The `ExprDataSource<T>` trait connects expressions to a live database. Both calls below read a
`product` table, so the block is compiled but not run:

```rust,no_run
# use vantage_expressions::{AssociatedQueryable, ExprDataSource};
# use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
# use vantage_sql::sqlite_expr;
# use vantage_core::Context;
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
# let expr = sqlite_expr!("SELECT name FROM product");
// Execute directly
let result: AnySqliteType = db.execute(&expr).await?;

// Associate with an expected return type
let count: i64 = db.associate::<i64>(sqlite_expr!("SELECT COUNT(*) FROM product"))
    .get().await?;
# let _ = (result, count);
# Ok(())
# }
```

`AssociatedExpression<'a, DS, T, R>` carries both the expression and a reference to the datasource.
Call `.get()` (from the `AssociatedQueryable` trait) to execute and convert in one step. The return
type `R` is checked at compile time.

---

## Deferred expressions — cross-database values

Sometimes a query on one database needs a value from another. `defer()` wraps a query as a closure
that resolves at execution time. The block runs both queries against tables it doesn't create, so
it is compiled but not run:

```rust,no_run
# use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
# use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
# use vantage_sql::sqlite_expr;
# use vantage_core::Context;
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let config_db = SqliteDB::connect("sqlite:config.db").await.context("connect")?;
# let shop_db = SqliteDB::connect("sqlite:shop.db").await.context("connect")?;
// Query config_db for a threshold — but don't execute yet
let threshold = config_db.defer(
    sqlite_expr!("SELECT value FROM config WHERE key = {}", "min_price")
);

// Use the deferred value in a query against shop_db
let expensive = Expression::<AnySqliteType>::new(
    "SELECT name FROM product WHERE price >= {}",
    vec![ExpressiveEnum::Deferred(threshold)],
);
let result = shop_db.execute(&expensive).await?;
// 1. Resolves deferred → calls config_db, gets 150
// 2. Binds 150 as a scalar parameter
// 3. Executes against shop_db
# let _ = result;
# Ok(())
# }
```

This is not a subquery — the deferred query runs first, produces a concrete value, and that value
gets bound as a regular parameter.

---

## Selectable — the query builder interface

The `Selectable<T>` trait is the standard interface for building SELECT queries. Each persistence
provides its own SELECT struct (`SqliteSelect`, `SurrealSelect`, `PostgresSelect`) implementing this
trait:

```rust
# use vantage_expressions::{Order, Selectable};
# use vantage_sql::primitives::identifier::ident;
# use vantage_sql::sqlite::statements::SqliteSelect;
# use vantage_sql::sqlite_expr;
let select = SqliteSelect::new()
    .with_source("product")
    .with_field("name")
    .with_field("price")
    .with_condition(sqlite_expr!("{} = {}", (ident("is_deleted")), false))
    .with_order(sqlite_expr!("{}", (ident("price"))), Order::Asc)
    .with_limit(Some(10), None);
# let _ = select;
```

**Builder methods** come free from the trait — `with_source`, `with_field`, `with_condition`,
`with_order` (an expression and an `Order`: `Order::Asc`, `Order::Desc`, optionally
`.nulls_last()`), `with_limit`. You only implement the mutating methods (`add_field`,
`add_where_condition`, etc.).

Aggregate shortcuts clone the query and replace fields:

```rust
# use vantage_expressions::Selectable;
# use vantage_sql::primitives::identifier::ident;
# use vantage_sql::sqlite::statements::SqliteSelect;
# use vantage_sql::sqlite_expr;
# let select = SqliteSelect::new().with_source("product").with_field("name");
let count = select.as_count();                              // SELECT COUNT(*) FROM ...
let total = select.as_sum(sqlite_expr!("{}", (ident("price")))); // SELECT SUM("price") FROM ...
# let _ = (count, total);
```

---

## SelectableDataSource — wiring it up

`SelectableDataSource<T>` connects the query builder to execution: it names the backend's SELECT
type, hands out an empty one, and runs a finished one. Here it is for `ShopDB`, a wrapper around
a SQLite connection, mirroring the impl `vantage-sql` ships for `SqliteDB`:

```rust
# use vantage_core::Result;
# use vantage_expressions::traits::datasource::DataSource;
# use vantage_expressions::{ExprDataSource, Expressive, SelectableDataSource};
# use vantage_sql::prelude::SqliteCondition;
# use vantage_sql::sqlite::statements::SqliteSelect;
# use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
#
# pub struct ShopDB(SqliteDB);
#
# impl DataSource for ShopDB {}
#
impl SelectableDataSource<AnySqliteType, SqliteCondition> for ShopDB {
    type Select = SqliteSelect;

    fn select(&self) -> Self::Select {
        SqliteSelect::new()
    }

    async fn execute_select(&self, select: &Self::Select) -> Result<Vec<AnySqliteType>> {
        let result = self.0.execute(&select.expr()).await?;
        // The result is an array of row objects: one value per row.
        match result.value() {
            ciborium::Value::Array(rows) => {
                Ok(rows.iter().map(|row| AnySqliteType::untyped(row.clone())).collect())
            }
            _ => Ok(vec![result]),
        }
    }
}
```

Once implemented, `table.select()` returns your vendor-specific builder pre-populated with the
table's columns, conditions, and ordering — ready for execution or further customisation. The full
walkthrough for a new backend is [Step 4: Query Builder](./new-persistence/step4-query-builder.md).

---

## Expressive trait

Anything that implements `Expressive<T>` can be used inside an expression. This includes:

- **Columns** — `table["price"]`
- **Operations** — `table["price"].gt(100)`
- **Identifiers** — `ident("name")`
- **Query builders** — `select.expr()`
- **Sort orders** — `table["name"].desc()`
- **Scalar values** — `42i64`, `"hello"`, `true`
- **Closures** — that's what `defer()` returns

You can implement `Expressive<T>` for your own types to make them composable into the expression
system.

Because conditions are themselves `Expressive`, operations **chain across type boundaries**:
`price.gt(10)` returns a `SqliteCondition`, and `.eq(false)` works on it — reading
"(price > 10) = 0". Type safety is enforced on the *first* operation, at the column level; once
you hold a condition, any value compatible with the backend's any-type is accepted
(`price.gt(10).eq("foobar")` compiles by design). Columns of the same type compare directly
(`price.eq(price.clone())`), and operations take ownership of their arguments — clone a column
you plan to reuse across conditions.

```rust
# use vantage_sql::prelude::*;
let price = Column::<i64>::new("price");
let cond = price.gt(10i64).eq(false);
assert_eq!(cond.into_expr().preview(), "price > 10 = 0");
```

## Checklist

To build and run a query with expressions:

1. **Pick the vendor macro** for your backend — `sqlite_expr!`, `postgres_expr!`, `mysql_expr!`,
   `surreal_expr!` — so every parameter carries that backend's type marker.
2. **Compose** with parenthesised arguments, `ident(..)` for names, and `Expression::from_vec`
   for lists.
3. **Build SELECTs** through `Selectable`: `with_source`, `with_field`, `with_condition`,
   `with_order(expr, Order::Asc)`, `with_limit`, and the `as_count`/`as_sum` shortcuts.
4. **Execute** through `ExprDataSource`: `execute` for a raw result, `associate::<R>(..).get()` for
   a typed one, `defer` for a value one database computes for another.
