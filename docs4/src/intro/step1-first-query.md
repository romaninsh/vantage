# SQLite and the Query Builder

Vantage is a big framework. It covers SQL databases, SurrealDB, MongoDB, CSV files, REST APIs — and
ties them all together with a shared type system, expression engine, and data abstraction layer.

We'll get to all of that. But right now, let's start with something familiar: **building SQL
queries**.

```admonish info title="What is a Query Builder?"
A query builder is a tool that assembles SQL from composable parts instead of string concatenation.
You've probably seen this pattern before:

- **Knex.js** (JavaScript) — `knex('users').where('age', '>', 18).select('name')`
- **SQLAlchemy Core** (Python) — `select(users.c.name).where(users.c.age > 18)`
- **JOOQ** (Java) — `dsl.select(USERS.NAME).from(USERS).where(USERS.AGE.gt(18))`
- **Diesel** (Rust) — `users.filter(age.gt(18)).select(name)`

Vantage has its own query builder too. Each supported database gets a dedicated builder —
`SqliteSelect`, `PostgresSelect`, `SurrealSelect` — so you get the right quoting, parameter
binding, and dialect features for your target.
```

For this chapter we'll use SQLite. It's lightweight, needs no server, and works with a plain file on
disk.

```admonish example title="Goals for this chapter"
By the end of this page you'll be able to:

1. Connect to an SQLite database from Rust
2. Build SELECT queries with fields, conditions, sorting, and limits
3. Execute queries and read results
4. Run aggregates (COUNT, SUM) with one method call
5. Understand how Vantage keeps parameters separate from SQL (no injection risk)
```

---

## Set up

```admonish note title="Pre-release"
Vantage 0.4 crates aren't on crates.io yet. For now, clone the repo
(`git clone https://github.com/romaninsh/vantage.git`) and work inside it as shown below.
```

Create a new project inside the Vantage workspace:

```sh
cd vantage
mkdir learn-1 && cd learn-1
cargo init
cargo add vantage-sql --path ../vantage-sql --features sqlite
cargo add tokio --features full
```

Two dependencies — `vantage-sql` gives us the SQLite query builder and connection pool, `tokio`
provides the async runtime because all database operations are async.

### Create and populate a database

We'll make a small product catalog from scratch. Create `seed.sql` in your project root:

```sql
CREATE TABLE product (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    price INTEGER NOT NULL,
    is_deleted BOOLEAN NOT NULL DEFAULT 0
);

INSERT INTO product VALUES ('cupcake',  'Cupcake',           120, 0);
INSERT INTO product VALUES ('donut',    'Doughnut',          135, 0);
INSERT INTO product VALUES ('tart',     'Tart',              220, 0);
INSERT INTO product VALUES ('pie',      'Pie',               299, 0);
INSERT INTO product VALUES ('cookies',  'Cookies',           199, 0);
INSERT INTO product VALUES ('old_cake', 'Discontinued Cake',  80, 1);
```

Run it:

```sh
sqlite3 products.db < seed.sql
```

You now have `products.db` — 6 rows, 5 active and 1 deleted. Quick check:

```sh
sqlite3 products.db "SELECT name, price FROM product WHERE is_deleted = 0"
```

---

## Start with an async main

All database operations in Vantage are async, so we need a Tokio runtime. Replace `src/main.rs`
with:

```rust
use vantage_sql::prelude::*;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.report();
    }
}

async fn run() -> VantageResult<()> {
    println!("Ready!");
    Ok(())
}
```

A few things going on here:

- **`use vantage_sql::prelude::*`** brings in everything we need for this chapter — `SqliteDB`,
  `SqliteSelect`, the `sqlite_expr!` macro, error types, and the traits that make builder and
  execution methods work.
- **`VantageResult<()>`** is Vantage's own Result type. It uses `VantageError`, which tracks context
  and error chains for readable diagnostics.
- **`e.report()`** prints the error in a structured format. We call it from `main()` because Rust's
  default `Result`-returning main uses `Debug` formatting, which is ugly. This pattern gives us
  clean error output instead.

Run `cargo run` to make sure it compiles.

---

## Connect to SQLite

Add this inside `run()`:

```rust
let db = SqliteDB::connect("sqlite:products.db?mode=ro")
    .await
    .context("Failed to connect to products.db")?;
```

`SqliteDB` wraps an sqlx connection pool. The connection string is an
[sqlx URL](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html) — `?mode=ro`
opens read-only, which is all we need for now.

```admonish tip title="Already have an sqlx pool?"
If you're adding Vantage to an existing project that already has a `SqlitePool`, wrap it directly:

~~~rust
let db = SqliteDB::new(existing_pool);
~~~

The reverse works too — `db.pool()` gives you the underlying `SqlitePool`, although Vantage
expressions will eliminate any need to execute queries directly.
```

```admonish info title=".context() — readable errors"
`.context()` wraps any error with a human-readable message. If the database file doesn't exist,
instead of a raw sqlx error you get:

~~~text
Error: Failed to connect to products.db
│
╰─▶ error returned from database: (code: 14) unable to open database file
~~~

You'll see `.context()` used throughout Vantage code. It comes from `VantageError` and works on
any `Result` with a standard error type.
```

---

## Build a SELECT

[`SqliteSelect`](vantage_sql::sqlite::statements::SqliteSelect) is the query builder for SQLite. Other persistences have their own —
[`PostgresSelect`](vantage_sql::postgres::statements::PostgresSelect),
[`MongoSelect`](vantage_mongodb::select::MongoSelect) — and they all implement the
[`Selectable`](vantage_expressions::Selectable) trait, so the interface is identical apart from
vendor-specific extensions. None of them need a database connection — they're just structs that
accumulate query parts. You build them with a chain of `.with_*()` calls:

```rust
let select = SqliteSelect::new()
    .with_source("product")
    .with_field("name")
    .with_field("price");

println!("{}", select.preview());
// SELECT "name", "price" FROM "product"
```

`.preview()` renders the final SQL as a string — handy for debugging, but never used for execution.

```admonish info title="Builder pattern"
`.with_*()` consumes the builder and returns a new one. Call `.with_field()` as many times as you
need; skip it entirely for `SELECT *`.
Every `.with_*()` method has a corresponding `.add_*()` that mutates in place instead of
consuming. Use whichever fits your code:

~~~rust
// Builder style
let select = SqliteSelect::new()
    .with_source("product")
    .with_field("name");

// Mutable style
let mut select = SqliteSelect::new();
select.add_source("product", None);
select.add_field("name");
~~~

Same result. The `.with_*()` style is nicer for one-shot construction, `.add_*()` is useful when
you're building a query conditionally in a loop.
```

---

## Execute it

```rust
let result = db.execute(&select.expr()).await?;
println!("{:?}", result);
```

Two steps here: `.expr()` turns the builder into an [`Expression`](vantage_expressions::Expression) —
Vantage's internal representation that keeps parameters separate from the SQL template. Then
[`db.execute()`](vantage_expressions::ExprDataSource::execute) sends it to the database.

The result is `AnySqliteType` — a type-tagged wrapper around whatever came back. The `Debug` output
isn't pretty, but you should see all 6 product rows in there.

```admonish tip title="When does Vantage hit the database?"
Only on `.await`. Everything before that — `with_source`, `with_field`, `with_condition` — is
synchronous struct manipulation. You always know when a database call happens because you typed
`.await`.
```

---

## Adding conditions

Our database has a soft-delete flag — `old_cake` has `is_deleted = 1`. Let's filter it out:

```rust
let condition = sqlite_expr!("\"is_deleted\" = {}", false);

let select = SqliteSelect::new()
    .with_source("product")
    .with_field("name")
    .with_field("price")
    .with_condition(condition);

println!("{}", select.preview());
// SELECT "name", "price" FROM "product" WHERE "is_deleted" = 0
```

`sqlite_expr!` creates an [`Expression`](vantage_expressions::Expression) — a SQL template with
typed, bound parameters. That `{}` is **not** Rust's `format!`: the value `false` is stored
separately and bound through sqlx's parameterized query interface at execution time — no injection
risk, ever. The preview shows it inline for readability.

In SQL persistences, a condition is just an expression. When you pass it to `.with_condition()`,
it's nested inside the select's own expression tree. At execution time the whole tree is
[flattened](vantage_expressions::ExpressionFlattener) into a single template + parameter list that
the database driver can bind safely.

Run it — you should see 5 rows, with "Discontinued Cake" filtered out.

```admonish info title="Types and persistence rendering"
Notice that you passed `false` but the preview shows `0`. SQLite has no native boolean — Vantage's
`SqliteType` implementation for `bool` converts it to an integer automatically. PostgreSQL would
render `FALSE` instead. Each persistence maps Rust types to the [correct native representation](../sql/type-conversions.md).

The `{}` parameter accepts any type that implements `SqliteType`: `bool`, `i64`, `f64`, `String`,
`chrono::NaiveDate`, `Option<T>`, and more. You can implement `SqliteType` for your own types too.
See [Persistence-aligned Type System](../type-system.md) for details.
```

### Identifiers and operators

Writing `\"is_deleted\"` in a raw expression works, but there's a cleaner way.
[`ident()`](vantage_sql::primitives::identifier::ident) creates a quoted
[`Identifier`](vantage_sql::primitives::identifier::Identifier), then chain an
[`Operation`](vantage_table::operation::Operation) like `.eq()` to build the condition:

```rust
let condition = ident("is_deleted").eq(false);
```

Same result, but the quoting is handled for you and the code reads more naturally. Other operators
— `.gt()`, `.lt()`, `.ne()`, `.in_()` — work the same way.

```admonish info title="Identifiers and operators are persistence-aware"
`ident()` quotes differently per backend — `"is_deleted"` for SQLite/Postgres,
`` `is_deleted` `` for MySQL. Operations can also differ between persistences; for example
[`in_list()`](vantage_table::operation::Operation::in_list) expands to `IN (?, ?, ?)` for SQL
but would map to native operators in other backends.
```

Multiple conditions combine with AND:

```rust
let select = SqliteSelect::new()
    .with_source("product")
    .with_field("name")
    .with_condition(ident("is_deleted").eq(false))
    .with_condition(ident("price").gt(150));
// ... WHERE "is_deleted" = 0 AND "price" > 150
```

---

## Sorting and limits

```rust
let select = SqliteSelect::new()
    .with_source("product")
    .with_field("name")
    .with_field("price")
    .with_order(sqlite_expr!("\"price\""), Order::Desc)
    .with_limit(Some(3), None);

println!("{}", select.preview());
// SELECT "name", "price" FROM "product" ORDER BY "price" DESC LIMIT 3
```

The second argument to `with_limit` is the offset — `None` means start from the beginning.

---

## Aggregates

Counting rows and summing values are so common that `Selectable` provides shortcuts. These methods
clone the query, strip the field list, and replace it with an aggregate:

```rust
let base = SqliteSelect::new()
    .with_source("product")
    .with_condition(sqlite_expr!("\"is_deleted\" = {}", false));

println!("{}", base.as_count().preview());
// SELECT COUNT(*) FROM "product" WHERE "is_deleted" = 0

println!("{}", base.as_sum(sqlite_expr!("\"price\"")).preview());
// SELECT SUM("price") FROM "product" WHERE "is_deleted" = 0
```

Notice that `base` isn't consumed — `as_count()` and `as_sum()` clone internally. You can keep using
`base` for other things.

To execute an aggregate and get a Rust value back, use `db.associate::<T>()`. It wraps the
expression with an expected return type so execution and conversion happen in one step:

```rust
let count: i64 = db.associate::<i64>(base.as_count()).get().await?;
println!("Active products: {count}");

let total: i64 = db.associate::<i64>(
    base.as_sum(sqlite_expr!("\"price\""))
).get().await?;
println!("Total price of active products: {total}");
```

If the database returns something that can't be converted to `i64`, you get an error — not garbage.

---

## Putting it together

Here's a small program that lists products from our database. It accepts an optional `--min-price`
argument to filter by price:

```rust
use vantage_sql::prelude::*;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.report();
    }
}

async fn run() -> VantageResult<()> {
    let db = SqliteDB::connect("sqlite:products.db?mode=ro")
        .await
        .context("Failed to connect to products.db")?;

    let mut select = SqliteSelect::new()
        .with_source("product")
        .with_field("name")
        .with_field("price")
        .with_condition(sqlite_expr!("\"is_deleted\" = {}", false))
        .with_order(sqlite_expr!("\"price\""), Order::Asc);

    // Optional filter
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--min-price") {
        if let Some(val) = args.get(pos + 1) {
            let min: i64 = val.parse().context("--min-price must be a number")?;
            select = select.with_condition(sqlite_expr!("\"price\" >= {}", min));
        }
    }

    // Show count
    let count: i64 = db.associate::<i64>(select.as_count()).get().await?;
    println!("{count} products found\n");

    // List them
    let result = db.execute(&select.expr()).await?;
    println!("{:?}", result);

    Ok(())
}
```

Try it:

```sh
cargo run
# 5 products found
# AnySqliteType { value: Array([...Cupcake...Doughnut...Cookies...Tart...Pie...]), ... }

cargo run -- --min-price 200
# 2 products found
# AnySqliteType { value: Array([...Tart...Pie...]), ... }
```

The output is raw `Debug` — not pretty, but it proves the query builder and conditions are working.
We'll improve the output in later chapters when we introduce typed entities.

---

## What we covered

| Concept                       | What it does                                              |
| ----------------------------- | --------------------------------------------------------- |
| `use vantage_sql::prelude::*` | Brings in all essentials — types, traits, macros          |
| `SqliteSelect`                | Builds SELECT queries via builder pattern                 |
| `sqlite_expr!`                | Creates expressions with typed, bound parameters          |
| `db.execute()`                | Runs an expression, returns raw results                   |
| `db.associate::<T>()`         | Executes and converts to a Rust type in one step          |
| `.as_count()` / `.as_sum()`   | Aggregate shortcuts — clone the query, replace the fields |
| `.preview()`                  | Shows the rendered SQL for debugging                      |

Everything so far is manual query building. You pick the table, the fields, the conditions. That's
flexible, but it's also a lot of repetition if you're always querying the same tables with the same
columns.

In the next chapter we'll start removing that repetition.
