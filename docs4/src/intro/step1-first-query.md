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
2. Build SELECT queries with fields and conditions
3. Execute queries and read results
4. Convert results into `Vec<Record>` with typed field access
5. Run aggregates (COUNT, SUM) with one method call
6. Understand how Vantage keeps parameters separate from SQL (no injection risk)
```

---

## Set up

Create a new project:

```sh
cargo init learn-1 && cd learn-1
cargo add vantage-sql --features sqlite
cargo add vantage-expressions
cargo add tokio --features full
```

Three dependencies — `vantage-sql` gives us the SQLite query builder and connection pool,
`vantage-expressions` is needed by the `sqlite_expr!` macro, and `tokio` provides the async
runtime because all database operations are async.

## Create and populate a database

We'll make a small product catalog from scratch. Create `seed.sql` in your project root:

```sql
CREATE TABLE product (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    price INTEGER NOT NULL,
    category_id INTEGER,
    is_deleted BOOLEAN NOT NULL DEFAULT 0
);

INSERT INTO product VALUES (1, 'Cupcake',           120, 1, 0);
INSERT INTO product VALUES (2, 'Doughnut',          135, 1, 0);
INSERT INTO product VALUES (3, 'Tart',              220, 2, 0);
INSERT INTO product VALUES (4, 'Pie',               299, 2, 0);
INSERT INTO product VALUES (5, 'Cookies',           199, 1, 0);
INSERT INTO product VALUES (6, 'Discontinued Cake',  80, 1, 1);
INSERT INTO product VALUES (7, 'Sourdough Loaf',    350, 3, 0);

CREATE TABLE category (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

INSERT INTO category VALUES (1, 'Sweet Treats');
INSERT INTO category VALUES (2, 'Pastries');
INSERT INTO category VALUES (3, 'Breads');
```

Run it:

```sh
sqlite3 products.db < seed.sql
```

You now have `products.db` — 7 products (6 active, 1 deleted) across 3 categories. Quick check:

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

[`SqliteSelect`](vantage_sql::sqlite::statements::SqliteSelect) is the query builder for SQLite.
Other persistences have their own —
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

Two steps here: `.expr()` turns the builder into an [`Expression`](vantage_expressions::Expression)
— Vantage's internal representation that keeps parameters separate from the SQL template. Then
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

## Typed columns and operators

Writing `\"is_deleted\"` in a raw expression works, but there's a cleaner way.
[`Column<T>`](vantage_table::column::core::Column) creates a typed column reference, then chain an
[`SqliteOperation`](vantage_sql::sqlite::operation::SqliteOperation) like `.eq()` to build the
condition:

```rust
let is_deleted = Column::<bool>::new("is_deleted");
let condition = is_deleted.eq(false);
```

Same result, but the type parameter `<bool>` ensures you can only compare against matching types.
Try `is_deleted.eq(42)` — it won't compile. Other operators — `.gt()`, `.lt()`, `.ne()`, `.in_()` —
enforce the same type safety.

The result is a `SqliteCondition` — the backend's native condition type — ready to be passed
directly to `.with_condition()`.

```admonish info title="Type safety and backend-specific operations"
Each SQL backend has its own operation trait — `SqliteOperation`, `PostgresOperation`,
`MysqlOperation` — imported automatically via the prelude. These traits are
blanket-implemented for any `Expressive<T>` where `T: Into<AnySqliteType>`, so typed columns
(`Column<i64>`, `Column<bool>`, etc.) all get `.eq()`, `.gt()`, and friends for free.

The operation produces a `SqliteCondition` that wraps `Expression<AnySqliteType>`. Since the
condition type itself implements `Expressive<AnySqliteType>`, you can **chain** operations
across type boundaries:

~~~rust
let price = Column::<i64>::new("price");
price.gt(10).eq(false)  // => (price > 10) = 0
~~~

Here `.gt(10)` returns a `SqliteCondition`, and `.eq(false)` works on it because `bool` is
`Expressive<AnySqliteType>`. Try `price.gt(10).eq("foobar")` — surprisingly, this compiles
too. That's by design: type safety is enforced on the **first** operation (the column level),
but once you have a `SqliteCondition`, any `AnySqliteType`-compatible value is accepted.

You can also compare columns of the same type: `price.eq(price.clone())` compiles. But
`price.eq(is_deleted)` won't — `Column<bool>` isn't `Expressive<i64>`.

The `.clone()` is needed because operations take ownership of their arguments — values are
stored inside the `Expression` tree until the query is executed. If you plan to reuse a
column in multiple conditions, clone it or create a fresh `Column::new()`.
```

Multiple conditions combine with AND:

```rust
let is_deleted = Column::<bool>::new("is_deleted");
let price = Column::<i64>::new("price");

let select = SqliteSelect::new()
    .with_source("product")
    .with_field("name")
    .with_condition(is_deleted.eq(false))
    .with_condition(price.gt(150));
// ... WHERE "is_deleted" = 0 AND "price" > 150
```

```admonish info title="Primitives for untyped access"
`sqlite_ident()` is one of several **primitives** — reusable building blocks for SQL expressions.
They handle quoting, escaping, and vendor-specific syntax. To use them:

~~~rust
use vantage_sql::sqlite::sqlite_ident as ident;

let condition = ident("is_deleted").eq(false);
~~~

Each backend has its own typed identifier: `sqlite_ident()`, `pg_ident()`, `mysql_ident()`.
These return a backend-pinned wrapper so `.eq()`, `.gt()`, etc. work without ambiguity.

There is also a generic `ident()` that works when the backend type can be inferred from
context — for example, inside `sqlite_expr!()` or when passed to a method that expects
a specific `Expressive<AnySqliteType>`. Use the typed variant when calling operations
directly.

Primitives are not part of the prelude — import them when needed. Besides identifiers, you get
`Fx` (function calls), `Case`, `Concat`, `Interval`, and more. See the
[Primitives reference](../sql/primitives.md) for the full list.

In Vantage, primitives, query builders, Column, Conditions and even native types
like i64 and bool - all implement Expressive<T>, where T= your databases AnyType
(for Sqlite its AnySqliteType - for MongoDB - AnyMongoType) - making them
eligible to be parameters of expressions.
```

---

## Working with Any-types

[`Record<V>`](vantage_types::Record) is an ordered map (`IndexMap<String, V>`) — one record per row,
with column names as keys. It lives in the `vantage-types` crate, so add that dependency and import
its prelude:

```sh
cargo add vantage-types --features serde
```

```rust
use vantage_types::prelude::*;
```

So far we've been printing raw `AnySqliteType` with `Debug`. That works for verifying queries, but
it's useless for real work. When `db.execute()` returns a multi-row result, the
[`AnySqliteType`](vantage_sql::sqlite::AnySqliteType) holds an array of maps internally. Convert it
to `Vec<Record<AnySqliteType>>` to work with individual rows:

```rust
let raw = db.execute(&select.expr()).await?;

let records = Vec::<Record<AnySqliteType>>::try_from(raw)
    .context("Failed to convert to records")?;

for rec in &records {
    let name: String = rec["name"].try_get::<String>().unwrap();
    let price: i64 = rec["price"].try_get::<i64>().unwrap();
    println!("{} — {} cents", name, price);
}
// Cupcake — 120 cents
// Doughnut — 135 cents
// ...
```

Access fields by column name with `rec["name"]`. Each value is still an `AnySqliteType`, so you call
`.try_get::<T>()` to extract a typed Rust value. If the type doesn't match (say you call
`.try_get::<i64>()` on a text column), you get `None` — no panics, no garbage.

```admonish warning title="serde_json::Value conversion"
If you need JSON-friendly records, call `.into_record()` on each `Record<AnySqliteType>`:

~~~rust
let json_rec: Record<serde_json::Value> = rec.into_record();
~~~

This is convenient for serialization, but `serde_json::Value` supports a narrower set of
types — you'll lose precision on `Decimal` and `chrono` types (dates become strings,
decimals become floats). Stick with `Record<AnySqliteType>` when you need full type fidelity.
```

Under the hood, each persistence has its own type system for storing values. SQLite uses
[CBOR](https://cbor.io/) — a compact binary format that preserves types like `Decimal`, `NaiveDate`,
and `NaiveDateTime` through tagged values. MongoDB uses [BSON](https://bsonspec.org/) natively. The
`AnySqliteType` / `AnyMongoType` wrappers hide these details — you interact with `.try_get::<T>()`
regardless of which persistence you're using. If you ever need to inspect the raw representation,
`.value()` gives you the underlying CBOR value:

```rust
let price_cbor = rec["price"].value();
println!("{:?}", price_cbor);
// Integer(Integer(120))
```

See [Persistence-aligned Type System](../type-system.md) for the full picture.

---

## Mapping rows to structs

Calling `.try_get::<T>()` on every field gets tedious. The `#[entity]` macro generates
[`TryFromRecord<AnySqliteType>`](vantage_types::TryFromRecord) for your struct, so conversion
happens in one call with no type information lost:

```rust
#[entity(SqliteType)]
struct Product {
    name: String,
    price: i64,
}

let raw = db.execute(&select.expr()).await?;
let records = Vec::<Record<AnySqliteType>>::try_from(raw)?;

for rec in records {
    let product = Product::from_record(rec)?;
    println!("{} — {} cents", product.name, product.price);
}
```

The macro needs `vantage-core` as a direct dependency (it's already a transitive dep through
`vantage-sql`, but the generated code references it in your crate):

```sh
cargo add vantage-core
```

The macro also supports multiple type systems in one attribute —
`#[entity(SqliteType, PostgresType, MongoType)]` generates a separate `TryFromRecord` impl for each
persistence. One struct, all backends.

```admonish info title="Serde alternative"
`#[entity]` converts each field directly through the persistence's type system — your struct
fields just need to implement `SqliteType`. Alternatively, you can convert a
`Record<AnySqliteType>` into `Record<serde_json::Value>` and use serde, but this may lose
type information (e.g. `Decimal` precision, date types):

~~~rust
#[derive(serde::Deserialize)]
struct Product {
    name: String,
    price: i64,
}

for rec in records {
    let json_rec: Record<serde_json::Value> = rec.into_record();
    let product = Product::from_record(json_rec)?;
}
~~~

For simple types like `String` and `i64` it's fine, but `Decimal` values lose precision and
dates become strings. Prefer `#[entity]` when your schema includes those types.
```

---

## Putting it together

Here's the complete `src/main.rs` — connect, query, convert to entities, print:

```rust
use vantage_sql::prelude::*;
use vantage_types::prelude::*;

#[entity(SqliteType)]
struct Product {
    name: String,
    price: i64,
}

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

    let select = SqliteSelect::new()
        .with_source("product")
        .with_field("name")
        .with_field("price")
        .with_condition(Column::<bool>::new("is_deleted").eq(false));

    let raw = db.execute(&select.expr()).await?;
    let records = Vec::<Record<AnySqliteType>>::try_from(raw)?;

    for rec in records {
        let p = Product::from_record(rec)?;
        println!("{:<12} {:>3} cents", p.name, p.price);
    }

    Ok(())
}
```

```sh
cargo run
# Cupcake      120 cents
# Doughnut     135 cents
# Tart         220 cents
# Pie          299 cents
# Cookies      199 cents
```

---

## What we covered

| Concept                                                              | What it does                                                                      | More info                                               |
| -------------------------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------- |
| [`SqliteSelect`](vantage_sql::sqlite::statements::SqliteSelect)      | Builds SELECT queries via builder pattern                                         | [`Selectable`](vantage_expressions::Selectable)         |
| [`Column::<T>`](vantage_table::column::core::Column)                 | Typed column reference — enforces matching operand types                          |                                                         |
| [`SqliteOperation`](vantage_sql::sqlite::operation::SqliteOperation) | Ext trait giving `.eq()`, `.gt()`, etc. → `SqliteCondition`                       |                                                         |
| `sqlite_expr!`                                                       | Creates expressions with typed, bound parameters                                  | [`Expression`](vantage_expressions::Expression)         |
| `db.execute()`                                                       | Runs an expression, returns [`AnySqliteType`](vantage_sql::sqlite::AnySqliteType) | [`ExprDataSource`](vantage_expressions::ExprDataSource) |
| [`Record<V>`](vantage_types::Record)                                 | Ordered map of column names to values — row-level access                          | `.try_get::<T>()`                                       |
| `#[entity(SqliteType)]`                                              | Generates lossless record-to-struct conversion                                    | [`TryFromRecord`](vantage_types::TryFromRecord)         |
