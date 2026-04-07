# vantage-sql

SQL backend for the [Vantage](https://github.com/romaninsh/vantage) persistence framework. Provides query builders, type systems, and execution for PostgreSQL, MySQL, and SQLite — with a single, vendor-agnostic API. Write your query logic once; `vantage-sql` renders correct SQL for each backend, including the parts where databases disagree on syntax.

## What Problem Does Vantage SQL Solve?

Imagine you're building an analytics dashboard in Rust. Your customers deploy it on their own infrastructure — some run PostgreSQL, some use MySQL, a few want embedded SQLite. Your application logic is the same, but the SQL isn't.

You reach for an ORM, but the moment you need a window function or a recursive CTE, you're writing raw SQL anyway. You try sqlx, but now you're maintaining three versions of every query. You consider abstracting over backends yourself, but that's a framework-sized project on its own.

The frustrating part is how *similar* these databases are. They all support joins, aggregates, subqueries, CTEs, window functions. The differences are mostly cosmetic — PostgreSQL quotes identifiers with `"`, MySQL uses backticks. SQLite concatenates strings with `||`, MySQL uses `CONCAT()`. They all extract JSON fields, just with different syntax. The semantics are the same; the spelling varies.

## Vantage Query Builder

Vantage is primarily a table-level abstraction framework for Rust — it gives you typed entities, relationships, and data operations across multiple backends. But underneath that lives a powerful query builder, and that's what this article is about.

Here's what the query builder gives you:

1. **Familiar select builder** — `PostgresSelect`, `MysqlSelect`, and `SqliteSelect` all implement the `Selectable` trait. Methods like `with_source`, `with_field`, `with_condition`, `with_join`, and `with_order` work identically across vendors. Learn one API, use it everywhere.

2. **Vendor-aware primitives** — small building blocks that render differently per backend, so you don't have to:
   - `ident("name")` — quotes as `"name"` on Postgres/SQLite, `` `name` `` on MySQL
   - `ternary(condition, true_val, false_val)` — renders as `IIF` / `IF` / `CASE WHEN`
   - `concat_sql!(a, b, c)` — renders as `||` or `CONCAT()`
   - `DateFormat::new(col, "%Y-%m")` — renders as `STRFTIME` / `DATE_FORMAT` / `TO_CHAR`
   - `JsonExtract::new(col, "field")` — renders as `JSON_EXTRACT` / `->>`

3. **Composable expressions** — primitives nest inside each other, inside joins, inside subqueries. You can mix vendor-aware primitives with raw expressions when you need something specific.

4. **Standard SQL operations** — `ident("salary").gt(50000.0)`, `.eq()`, `.in_()` — the `Operation` trait works on any `Expressive` type, so conditions read naturally.

This guide walks through building real queries for an analytics dashboard — starting with the basics and working up to the parts where vendor differences actually bite.

## Your First Query

The dashboard needs a user list. Admins with a salary over 50k, sorted by name. In raw SQL:

```sql
SELECT id, name, email FROM users
WHERE role = 'admin' AND salary > 50000.0
ORDER BY name
```

Here's how you build it with Vantage:

```rust
let select = PostgresSelect::new()
    .with_source("users")
    .with_field("id")
    .with_field("name")
    .with_field("email")
    .with_condition(ident("role").eq("admin"))
    .with_condition(ident("salary").gt(50000.0f64))
    .with_order(ident("name"), true);
```

A few things to notice:

**`ident("role")`** creates a quoted identifier. On Postgres this becomes `"role"`, on MySQL it becomes `` `role` ``. You never write quotes yourself — `ident` handles it based on the vendor type. Importantly, `ident` is implemented once — it's not a Postgres thing or a MySQL thing. It's a context-aware primitive: it looks at what kind of query it's being used in and renders accordingly. This matters because if you build your own primitives, they'll work with every vendor automatically using the same pattern.

**`Expressive` — the universal trait.** `ident("role")` returns a struct that implements `Expressive`. So does a `PostgresSelect`, a raw expression, and every other primitive in this guide. Builder methods like `with_condition`, `with_order`, and `with_expression` all accept `impl Expressive<T>` — so anything that implements the trait fits anywhere an expression is expected. This is how everything composes.

Most SQL builders and template engines are single-dimensional — you provide a flat template string and a list of parameters. Vantage expressions are recursive. An identifier can be placed inside a condition, that condition inside an `OR`, that `OR` inside a `with_condition` on a `MysqlSelect` — and only at the final rendering step does the identifier discover it's inside a MySQL query and produce backtick quoting. The structure is assembled first; the vendor-specific rendering happens last.

```rust
// Build pieces independently — no vendor commitment yet
let role_check = ident("role").eq("admin");
let salary_check = ident("salary").gt(50000.0f64);

// Combine into a compound condition
let condition = mysql_expr!("{} AND {}", (role_check), (salary_check));

// Use in a MySQL query — now ident knows to use backticks
let select = MysqlSelect::new()
    .with_source("users")
    .with_condition(condition);

// → WHERE `role` = 'admin' AND `salary` > 50000.0
```

Expressions can also contain deferred closures — async functions that resolve at execution time — but more on that later.

**`.eq("admin")`** comes from the `Operation` trait, which is blanket-implemented for anything `Expressive`. So any identifier, column, or expression gets `.eq()`, `.gt()`, `.gte()`, `.lt()`, `.ne()`, and `.in_()` for free. The string `"admin"` is automatically treated as a quoted literal — `'admin'` in the output SQL.

**`.with_condition()`** called twice produces `WHERE ... AND ...`. Conditions compose naturally.

**The select type determines the vendor.** `PostgresSelect::new()` produces PostgreSQL syntax. Swap it for `MysqlSelect::new()` and the same builder chain produces MySQL syntax. The builder methods are identical — they come from the `Selectable` trait.

Now — everything so far has been standard SQL that just needs different quoting. Where it gets interesting is when databases genuinely disagree on syntax.

## When Databases Disagree

### Inline Conditionals with `ternary`

The dashboard needs to label each user as an admin or not. In SQL, this is a simple inline conditional — but every database spells it differently:

```sql
-- SQLite
IIF(role = 'admin', 'Yes', 'No')

-- MySQL
IF(`role` = 'admin', 'Yes', 'No')

-- PostgreSQL
CASE WHEN "role" = 'admin' THEN 'Yes' ELSE 'No' END
```

Three syntaxes, same semantics. In Vantage, one call:

```rust
ternary(
    ident("role").eq("admin"),
    "Yes",
    "No",
).with_alias("is_admin")
```

The `ternary` primitive takes a condition, a true value, and a false value. All three are `impl Expressive` — so `"Yes"` and `"No"` work as SQL-injection-safe string literals, identifiers work as quoted columns, and other primitives nest naturally.

Say you're building a report that shows when each order was completed — but some orders are still open. You want a formatted date or the text "ongoing":

```rust
ternary(
    expr_any!("{} IS NOT NULL", (ident("completed_at").dot_of("o"))),
    DateFormat::new(ident("completed_at").dot_of("o"), "%Y-%m"),
    "ongoing",
).with_alias("completed")
```

There are two vendor-aware primitives here — `ternary` and `DateFormat` — nested together. Each renders independently for the target database:

```sql
-- SQLite
IIF("o"."completed_at" IS NOT NULL, STRFTIME('%Y-%m', "o"."completed_at"), 'ongoing')

-- MySQL
IF(`o`.`completed_at` IS NOT NULL, DATE_FORMAT(`o`.`completed_at`, '%Y-%m'), 'ongoing')

-- PostgreSQL
CASE WHEN "o"."completed_at" IS NOT NULL
  THEN TO_CHAR("o"."completed_at", 'YYYY-MM') ELSE 'ongoing' END
```

Notice `expr_any!` for the NULL check — it creates a raw expression without committing to a vendor. The type is inferred from context: inside a `MysqlSelect` it becomes MySQL, inside a `PostgresSelect` it becomes PostgreSQL. Use `expr_any!` when you need a SQL fragment that doesn't have its own primitive yet.

And you may have noticed — we just introduced `DateFormat`.

### Date Formatting with `DateFormat`

The monthly revenue report groups orders by year-month. Every database can do this, but none of them agree on how:

```sql
-- SQLite
STRFTIME('%Y-%m', "o"."created_at")

-- MySQL
DATE_FORMAT(`o`.`created_at`, '%Y-%m')

-- PostgreSQL
TO_CHAR("o"."created_at", 'YYYY-MM')
```

Different function names, different argument order (SQLite puts the format first), and PostgreSQL uses entirely different format tokens — `YYYY` instead of `%Y`, `MM` instead of `%m`.

In Vantage, you use strftime-style tokens — the format Rust developers already know from `chrono` — and the primitive handles the rest:

```rust
let month = DateFormat::new(ident("created_at").dot_of("o"), "%Y-%m");
```

The `DateFormat` primitive translates `%Y` → `YYYY` and `%m` → `MM` for PostgreSQL, adjusts the argument order for SQLite, and picks the right function name for each vendor. You learn one format syntax; the primitive speaks three.

This works naturally in a larger query — here's the revenue report:

```rust
let month = DateFormat::new(ident("created_at").dot_of("o"), "%Y-%m");
let revenue = Fx::new("sum", [ident("total").dot_of("o").expr()]);

let select = PostgresSelect::new()
    .with_source_as("orders", "o")
    .with_expression(month.clone(), Some("month".into()))
    .with_expression(
        Fx::new("round", [revenue.expr(), expr_any!("{}", 2i32)]),
        Some("monthly_revenue".into()),
    )
    .with_group_by(ident("month"))
    .with_order(ident("month"), false);
```

`Fx::new("sum", ...)` and `Fx::new("round", ...)` are the general-purpose function primitive — they uppercase the name and wrap the arguments. Unlike `DateFormat` or `ternary`, `Fx` renders the same on every vendor, which is fine for functions like `SUM`, `ROUND`, `COUNT`, and `AVG` that are genuinely universal.

## Building Your Own Primitive

You can probably guess that Vantage ships a primitive for string concatenation — SQLite and PostgreSQL use `||`, MySQL uses `CONCAT()`. But let's imagine for a moment that it didn't exist and you needed to build it yourself. This is the real power of the system: the pattern is simple enough that adding a new vendor-aware primitive takes minutes.

### Step 1: Define the struct

A primitive is just a struct that holds its arguments as `Expression<T>`:

```rust
#[derive(Debug, Clone)]
pub struct Concat<T: Debug + Display + Clone> {
    parts: Vec<Expression<T>>,
}

impl<T: Debug + Display + Clone> Concat<T> {
    pub fn new(parts: impl IntoVec<Expression<T>>) -> Self {
        Self { parts: parts.into_vec() }
    }
}
```

The struct is generic over `T` — it doesn't know or care which database it's targeting. It just holds expressions.

`IntoVec` is a convenience trait that lets `new()` accept a `Vec`, an array, or a slice — so callers can write `Concat::new([a, b, c])` without wrapping in `vec![]`. Small ergonomic detail, but it adds up when you're composing many primitives.

Notice that `new()` takes `Expression<T>`, not `impl Expressive<T>`. This means callers need to call `.expr()` on each argument. That's a deliberate trade-off — a `Vec` can only hold one type, and different primitives (`Identifier`, `&str`, `Fx`) are different types even though they all implement `Expressive`. The `concat_sql!` macro we'll show later removes this friction by calling `.expr()` automatically.

### Step 2: Implement `Expressive` per vendor

This is where the vendor-specific rendering lives. For SQLite and PostgreSQL, join the parts with `||`. For MySQL, wrap them in `CONCAT()`:

```rust
// SQLite
impl Expressive<AnySqliteType> for Concat<AnySqliteType> {
    fn expr(&self) -> Expression<AnySqliteType> {
        Expression::from_vec(self.parts.clone(), " || ")
    }
}

// MySQL
impl Expressive<AnyMysqlType> for Concat<AnyMysqlType> {
    fn expr(&self) -> Expression<AnyMysqlType> {
        let args = Expression::from_vec(self.parts.clone(), ", ");
        Expression::new("CONCAT({})", vec![ExpressiveEnum::Nested(args)])
    }
}

// PostgreSQL
impl Expressive<AnyPostgresType> for Concat<AnyPostgresType> {
    fn expr(&self) -> Expression<AnyPostgresType> {
        Expression::from_vec(self.parts.clone(), " || ")
    }
}
```

That's it. Three small impl blocks, each a few lines. `Expression::from_vec` joins a list of expressions with a separator. `Expression::new` wraps them in a template with `{}` placeholders.

### Step 3: Use it

Your new primitive composes with everything else — identifiers, literals, other primitives:

```rust
let breadcrumb = Concat::new(vec![
    ident("path").dot_of("dt").expr(),
    expr_any!("{}", " > "),
    ident("name").dot_of("d").expr(),
]);
```

```sql
-- SQLite / PostgreSQL
"dt"."path" || ' > ' || "d"."name"

-- MySQL
CONCAT(`dt`.`path`, ' > ', `d`.`name`)
```

Your primitive is a first-class citizen — no special registration, no plugin system. It just implements the trait.

> Vantage ships `Concat` along with a `concat_sql!` macro that calls `.expr()` on each argument automatically. But the implementation above is the real one — there's no hidden magic.

## What Else the Query Builder Can Do

This guide covered the fundamentals — selecting, filtering, composing expressions, and building vendor-aware primitives. But the query builder goes much further:

- **Joins** — `inner`, `left`, and subquery joins via `SelectJoin`. Qualified identifiers with `ident("name").dot_of("u")` get vendor-correct quoting throughout.
- **Aggregates and grouping** — `Fx::new("sum", ...)`, `with_group_by`, `with_having`. The universal `Fx` primitive handles any SQL function that's spelled the same everywhere.
- **Subqueries** — a `Select` is `Expressive`, so you can nest one inside another's `with_condition` (for `EXISTS`), `with_expression` (for scalar subqueries), or `with_join` (for derived tables).
- **CTEs** — `with_cte("name", select, recursive)` adds `WITH` / `WITH RECURSIVE` clauses. CTEs can reference each other.
- **Window functions** — `Window::new().partition_by(...).order_by(...)` with named windows, `ROW_NUMBER`, `RANK`, `LAG`/`LEAD`, `FIRST_VALUE`, and frame specs (`ROWS`, `RANGE`).
- **UNION / EXCEPT / INTERSECT** — `Union::new(select).union_all(other).except(third)` composes set operations.
- **JSON extraction** — `JsonExtract::new(col, "field")` renders as `JSON_EXTRACT(col, '$.field')` on SQLite/MySQL and `col->>'field'` on PostgreSQL. Paths, quoting, and operators all adapt.
- **DISTINCT, LIMIT, OFFSET** — the basics, available on every vendor through `with_distinct`, `with_limit`.

All of these compose through `Expressive`. A `DateFormat` inside a `ternary` inside a CTE inside a `UNION` — it all just works because every piece speaks the same trait.

## Beyond the Query Builder

The query builder is a powerful tool on its own, but it's only one layer of Vantage. The framework is built around a broader idea: **a cohesive persistence abstraction that works across fundamentally different data backends**.

You may have noticed `AnySqliteType`, `AnyMysqlType`, `AnyPostgresType` appearing throughout this guide. These aren't just marker types — they're part of a strongly-typed, vendor-specific type system with enforced boundaries. Each backend defines how Rust types map to its native types, how records are serialized, and how values cross the boundary between your application and the database.

On top of this type system, Vantage builds:

- **Entity tables** — define a struct, derive a table, and get typed CRUD operations. Columns know their types. Relationships between tables are first-class — one-to-many, many-to-many, with traversal built in.
- **Schemaless data** — not every backend has a schema. Vantage's `Record` type works equally well with typed columns and with arbitrary key-value data.
- **New backends in days, not months** — implementing full support for a new database engine (like SurrealDB) means defining a type system, an expression renderer, and a table source. The Persistence Guide walks through every step. The community has used this to add support for CSV files and REST APIs with progressive pagination.
- **Multi-backend applications** — `AnyTable` provides type-erased wrappers so your application logic can work against any backend. A CLI tool that queries PostgreSQL, SQLite, and a REST API in the same session isn't a special case — it's the normal way to use Vantage.
- **Polyglot interfaces** — because the abstraction is clean, it can be exposed to other languages. Mobile applications, Python scripts, and web frontends can all work with the same entity definitions through FFI or API boundaries.

The query builder gives you vendor-agnostic SQL. Vantage gives you vendor-agnostic *persistence*.
