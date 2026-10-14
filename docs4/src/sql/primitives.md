# SQL Primitives

Primitives are reusable building blocks for constructing SQL expressions. They handle quoting,
escaping, vendor-specific syntax, and logical composition so you don't have to.

Conditions built with typed columns and `SqliteOperation` cover simple comparisons, but real queries
need more — `OR` groups, function calls, string concatenation, date formatting. That's what
primitives are for.

Import them with:

```rust
use vantage_sql::primitives::*;
use vantage_sql::sqlite::sqlite_ident as ident;
```

Primitives are **not** part of the prelude — import them explicitly when needed. The second line
pins `ident` to SQLite; [`ident()`](#ident--identifiers) explains why the examples need it.

```admonish note title="Macros and structs"
Some primitives have convenience macros that accept a variable number of arguments and call
`.expr()` on each one automatically: `fx!` → `Fx`, `concat_!` → `Concat`. The macros are
syntactic sugar — if you need to build arguments programmatically (e.g. from a `Vec`), use
the underlying struct directly.
```

## `or_()` / `and_()` — Logical Combinators

By default, multiple calls to `.with_condition()` combine with `AND`. When you need `OR`, use
[`or_()`](vantage_sql::primitives::logical::or_):

```rust
use vantage_sql::primitives::*;
# use vantage_sql::sqlite::operation::SqliteOperation;
# use vantage_sql::sqlite::sqlite_ident as ident;

// role = 'admin' OR role = 'superuser'
let cond = or_(ident("role").eq("admin"), ident("role").eq("superuser"));
```

For nested logic, combine with [`and_()`](vantage_sql::primitives::logical::and_):

```rust
# use vantage_sql::primitives::*;
# use vantage_sql::sqlite::operation::SqliteOperation;
# use vantage_sql::sqlite::sqlite_ident as ident;
// (price > 100 AND in_stock = 1) OR (featured = 1)
let cond = or_(
    and_(ident("price").gt(100i64), ident("in_stock").eq(true)),
    ident("featured").eq(true),
);
```

Both return a [`ConditionGroup`](vantage_sql::primitives::logical::ConditionGroup), which writes
its own brackets and is `Expressive`, so it plugs directly into `.with_condition()`. A chain stays
flat: `a.or_(b).or_(c)` renders `(a OR b OR c)`.

## `ident()` — Identifiers

Creates a quoted column or table name. Quoting adapts per backend (`"` for SQLite/Postgres, `` ` ``
for MySQL).

```rust
# use vantage_sql::sqlite::sqlite_ident as ident;
let col = ident("price");                     // "price"
let qualified = ident("name").dot_of("u");    // "u"."name"
let aliased = ident("total").with_alias("t"); // "total" AS "t"
```

[`ident()`](vantage_sql::primitives::identifier::ident) is a shorthand for
[`Identifier::new()`](vantage_sql::primitives::identifier::Identifier::new). Every name is quoted,
and a quote character inside the name is doubled, so a name can't break out of its quotes.

Each backend also has a **typed** identifier:
[`sqlite_ident()`](vantage_sql::sqlite::sqlite_ident), `pg_ident()` (in `vantage_sql::postgres`)
and `mysql_ident()` (in `vantage_sql::mysql`). Each returns a wrapper that only renders for its own
backend.

### Why not the generic `ident()` everywhere?

The generic `Identifier` renders for every backend feature you enabled. With one feature on, that
is unambiguous. With two, `ident("price").gt(100i64)` could build a SQLite condition or a
PostgreSQL one, and the compiler stops with "type annotations needed". This book builds SQLite and
PostgreSQL, and most applications that talk to two databases do too.

The generic one still works where the backend is fixed by context: inside `sqlite_expr!()`, or
passed where a specific `Expressive<T>` is expected. Use a typed identifier when you call an
operation on it directly. The examples on this page import `sqlite_ident as ident`, so they read
the same as single-backend code.

## `fx!` — Function Calls

The [`fx!`](vantage_sql::fx) macro builds a SQL function call. Arguments are passed directly —
`.expr()` is called on each one automatically:

```rust
# use vantage_sql::sqlite::sqlite_ident as ident;
use vantage_sql::{fx, sqlite_expr};

let count = fx!("count", sqlite_expr!("*"));
// => COUNT(*)

let average = fx!("avg", ident("price"));
// => AVG("price")

// Multiple arguments
let nickname = fx!("coalesce", ident("nickname"), "anonymous");
// => COALESCE("nickname", 'anonymous')

// Nested
let rounded = fx!("round", fx!("avg", ident("price")), 2i64);
// => ROUND(AVG("price"), 2)
```

If you need to build arguments programmatically (e.g. from a `Vec`), use
[`Fx::new()`](vantage_sql::primitives::fx::Fx::new) directly:

```rust
# use vantage_expressions::{Expression, Expressive};
# use vantage_sql::primitives::*;
# use vantage_sql::sqlite::AnySqliteType;
# use vantage_sql::sqlite::sqlite_ident as ident;
# let columns = [ident("nickname"), ident("name")];
let args: Vec<Expression<AnySqliteType>> = columns.iter().map(|c| c.expr()).collect();
let f = Fx::new("coalesce", args);
```

## `ternary()` — Conditional Expression

Three-valued conditional. Renders as `IIF()` on SQLite, `IF()` on MySQL, and
`CASE WHEN ... THEN ... ELSE ... END` on PostgreSQL:

```rust
# use vantage_sql::primitives::*;
# use vantage_sql::sqlite::operation::SqliteOperation;
# use vantage_sql::sqlite::sqlite_ident as ident;
let expr = ternary(
    ident("stock").gt(0i64),
    "in stock",
    "sold out",
);
```

[`ternary()`](vantage_sql::primitives::ternary::ternary) is a shorthand for
[`Ternary::new()`](vantage_sql::primitives::ternary::Ternary::new).

## `Case` — CASE Expressions

For more than two branches, use [`Case`](vantage_sql::primitives::case::Case) to build a full
`CASE WHEN ... END` block:

```rust
# use vantage_sql::primitives::*;
# use vantage_sql::sqlite::operation::SqliteOperation;
# use vantage_sql::sqlite::sqlite_ident as ident;
let expr = Case::new()
    .when(ident("status").eq("active"), "yes")
    .when(ident("status").eq("banned"), "no")
    .else_("unknown");
```

## `concat_!` — String Concatenation

Concatenates expressions. Renders as `||` on SQLite/Postgres, `CONCAT()` on MySQL. The
[`concat_!`](vantage_sql::concat_) macro calls `.expr()` on each argument automatically:

```rust
# use vantage_sql::sqlite::sqlite_ident as ident;
use vantage_sql::concat_;

let full_name = concat_!(ident("first_name"), " ", ident("last_name"));
```

Use [`.ws()`](vantage_sql::primitives::concat::Concat::ws) for a separator — it accepts any
`Expressive<T>`, including string literals:

```rust
# use vantage_sql::concat_;
# use vantage_sql::sqlite::sqlite_ident as ident;
let listed = concat_!(ident("first_name"), ident("last_name")).ws(", ");
// SQLite:   "first_name" || ', ' || "last_name"
// MySQL:    CONCAT_WS(', ', `first_name`, `last_name`)
```

## `Interval` — Date Intervals

Portable date interval that adapts per backend:

```rust
# use vantage_sql::primitives::*;
let i = Interval::new(30, "day");
// SQLite:   30  (used with date functions)
// MySQL:    INTERVAL 30 DAY
// Postgres: INTERVAL '30 day'
```

[`Interval::new(amount, unit)`](vantage_sql::primitives::interval::Interval::new) takes the unit
as text: `day`, `week`, `month`, `year` or `hour`, singular or plural. Use the singular form when
the query may run on MySQL, which accepts only `DAY`, `MONTH` and so on. SQLite has no interval
type, so it renders the interval as a number of days: a week is 7, a month 30, a year 365, and
hours round to whole days.

## `date_format()` — Date Formatting

Portable strftime-style formatting. Translates format tokens per backend:

```rust
# use vantage_sql::primitives::*;
# use vantage_sql::sqlite::sqlite_ident as ident;
let formatted = date_format(ident("created_at"), "%Y-%m-%d");
// SQLite:   STRFTIME('%Y-%m-%d', "created_at")
// MySQL:    DATE_FORMAT("created_at", '%Y-%m-%d')
// Postgres: TO_CHAR("created_at", 'YYYY-MM-DD')
```

[`date_format()`](vantage_sql::primitives::date_format::date_format) is a shorthand for
[`DateFormat::new()`](vantage_sql::primitives::date_format::DateFormat::new). To skip token
translation and pass a native format string, build it with
[`DateFormat::raw_format()`](vantage_sql::primitives::date_format::DateFormat::raw_format)
instead.
