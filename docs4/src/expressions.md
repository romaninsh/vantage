# Expressions & Queries

Vantage builds queries without string concatenation. The `vantage-expressions` crate provides a
type-safe, composable expression system that works across all persistence backends — SQL, SurrealDB,
MongoDB, CSV, and anything you add yourself.

<!-- toc -->

---

## The core idea

An `Expression<T>` is a template string with typed parameters:

```rust
let expr = sqlite_expr!("SELECT {} FROM {} WHERE {} > {}",
    (ident("name")), (ident("product")), (ident("price")), 100i64);
// → SELECT "name" FROM "product" WHERE "price" > ?1  (with 100 bound as i64)
```

Parameters are never interpolated into the string. They're carried separately, each tagged with a
type marker from your persistence's type system. The bind layer uses these markers to call the right
driver method — `bind_i64`, `bind_str`, `bind_bool` — no guessing, no silent coercion.

Three kinds of parameters:

- **Scalar** — a typed value: `42i64`, `"hello"`, `true`
- **Nested** — another expression, composed into the template
- **Deferred** — a closure that executes later (cross-database resolution)

---

## Vendor macros

Each persistence provides a convenience macro that produces `Expression<AnyType>` with the correct
type wrapping:

```rust
let e = sqlite_expr!("SELECT * FROM product WHERE price > {}", 100i64);
let e = surreal_expr!("SELECT * FROM product WHERE price > {}", 100i64);
let e = postgres_expr!("SELECT * FROM product WHERE price > {}", 100i64);
let e = mysql_expr!("SELECT * FROM product WHERE price > {}", 100i64);
```

Same syntax, different type universes. The compiler ensures you can't accidentally mix a
`Expression<AnySqliteType>` into a SurrealDB query.

---

## Composing expressions

Expressions nest naturally. Parenthesised arguments call `.expr()` automatically:

```rust
let condition = sqlite_expr!("{} > {}", (ident("price")), 100i64);
let query = sqlite_expr!("SELECT {} FROM {} WHERE {}",
    (ident("name")), (ident("product")), (condition));
```

The `ExpressionFlattener` collapses all nesting into a single flat template with positional
parameters — each one still carrying its type marker.

For building lists (e.g. multi-row INSERT), use `Expression::from_vec`:

```rust
let row1 = sqlite_expr!("({}, {})", "tart", 220i64);
let row2 = sqlite_expr!("({}, {})", "pie", 299i64);
let rows = Expression::from_vec(vec![row1, row2], ", ");
```

---

## Identifier quoting

SQL identifiers need quoting — and each database uses different quote characters. The `Identifier`
struct handles this by implementing `Expressive<T>` for each backend type:

```rust
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

The `ExprDataSource<T>` trait connects expressions to a live database:

```rust
// Execute directly
let result: AnySqliteType = db.execute(&expr).await?;

// Associate with an expected return type
let count: i64 = db.associate::<i64>(sqlite_expr!("SELECT COUNT(*) FROM product"))
    .get().await?;
```

`AssociatedExpression<'a, DS, T, R>` carries both the expression and a reference to the datasource.
Call `.get()` to execute and convert in one step. The return type `R` is checked at compile time.

---

## Deferred expressions — cross-database values

Sometimes a query on one database needs a value from another. `defer()` wraps a query as a closure
that resolves at execution time:

```rust
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
```

This is not a subquery — the deferred query runs first, produces a concrete value, and that value
gets bound as a regular parameter.

---

## Selectable — the query builder interface

The `Selectable<T>` trait is the standard interface for building SELECT queries. Each persistence
provides its own SELECT struct (`SqliteSelect`, `SurrealSelect`, `PostgresSelect`) implementing this
trait:

```rust
let select = SqliteSelect::new()
    .with_source("product")
    .with_field("name")
    .with_field("price")
    .with_condition(sqlite_expr!("{} = {}", (ident("is_deleted")), false))
    .with_order(sqlite_expr!("{}", (ident("price"))), false)
    .with_limit(Some(10), None);
```

**Builder methods** come free from the trait — `with_source`, `with_field`, `with_condition`,
`with_order`, `with_limit`. You only implement the mutating methods (`add_field`,
`add_where_condition`, etc.).

Aggregate shortcuts clone the query and replace fields:

```rust
let count = select.as_count();                              // SELECT COUNT(*) FROM ...
let total = select.as_sum(sqlite_expr!("{}", (ident("price")))); // SELECT SUM("price") FROM ...
```

---

## SelectableDataSource — wiring it up

`SelectableDataSource<T>` connects the query builder to execution:

```rust
impl SelectableDataSource<AnySqliteType> for SqliteDB {
    type Select = SqliteSelect;

    fn select(&self) -> Self::Select { SqliteSelect::new() }
    async fn execute_select(&self, select: &Self::Select) -> Result<Vec<AnySqliteType>> {
        self.execute(&select.expr()).await
    }
}
```

Once implemented, `table.select()` returns your vendor-specific builder pre-populated with the
table's columns, conditions, and ordering — ready for execution or further customisation.

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
