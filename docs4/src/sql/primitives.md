# SQL Primitives

Primitives are reusable building blocks for constructing SQL expressions. They handle quoting,
escaping, vendor-specific syntax, and logical composition so you don't have to.

Conditions built with typed columns and `SqliteOperation` cover simple comparisons, but real queries
need more — `OR` groups, function calls, string concatenation, date formatting. That's what
primitives are for.

Import them with:

```rust
use vantage_sql::primitives::*;
```

Primitives are **not** part of the prelude — import them explicitly when needed.

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

// role = 'admin' OR role = 'superuser'
let cond = or_(ident("role").eq("admin"), ident("role").eq("superuser"));
```

For nested logic, combine with [`and_()`](vantage_sql::primitives::logical::and_):

```rust
// (price > 100 AND in_stock = 1) OR (featured = 1)
let cond = or_(
    and_(ident("price").gt(100), ident("in_stock").eq(true)),
    ident("featured").eq(true),
);
```

Both return `Expression<T>`, so they plug directly into `.with_condition()`.

## `ident()` — Identifiers

Creates a quoted column or table name. Quoting adapts per backend (`"` for SQLite/Postgres, `` ` ``
for MySQL).

```rust
let col = ident("price");                     // "price"
let qualified = ident("name").dot_of("u");    // "u"."name"
let aliased = ident("total").with_alias("t"); // "total" AS "t"
```

[`ident()`](vantage_sql::primitives::identifier::ident) is a shorthand for
[`Identifier::new()`](vantage_sql::primitives::identifier::Identifier::new). Reserved words and
names with spaces are quoted automatically.

## `fx!` — Function Calls

The [`fx!`](vantage_sql::fx) macro builds a SQL function call. Arguments are passed directly —
`.expr()` is called on each one automatically:

```rust
fx!("count", sqlite_expr!("*"))
// => COUNT(*)

fx!("avg", ident("price"))
// => AVG("price")

// Multiple arguments
fx!("coalesce", ident("nickname"), "anonymous")
// => COALESCE("nickname", 'anonymous')

// Nested
fx!("round", fx!("avg", ident("price")), 2i64)
// => ROUND(AVG("price"), 2)
```

If you need to build arguments programmatically (e.g. from a `Vec`), use
[`Fx::new()`](vantage_sql::primitives::fx::Fx::new) directly:

```rust
let args: Vec<Expression<AnySqliteType>> = columns.iter().map(|c| c.expr()).collect();
let f = Fx::new("coalesce", args);
```

## `ternary()` — Conditional Expression

Three-valued conditional. Renders as `IIF()` on SQLite, `IF()` on MySQL, and
`CASE WHEN ... THEN ... ELSE ... END` on PostgreSQL:

```rust
let expr = ternary(
    ident("stock").gt(0),
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
let expr = Case::new()
    .when(ident("status").eq("active"), "yes")
    .when(ident("status").eq("banned"), "no")
    .else_("unknown");
```

## `concat_!` — String Concatenation

Concatenates expressions. Renders as `||` on SQLite/Postgres, `CONCAT()` on MySQL. The
[`concat_!`](vantage_sql::concat_) macro calls `.expr()` on each argument automatically:

```rust
concat_!(ident("first_name"), " ", ident("last_name"))
```

Use [`.ws()`](vantage_sql::primitives::concat::Concat::ws) for a separator — it accepts any
`Expressive<T>`, including string literals:

```rust
concat_!(ident("first_name"), ident("last_name")).ws(", ")
// SQLite:   "first_name" || ', ' || "last_name"
// MySQL:    CONCAT_WS(', ', `first_name`, `last_name`)
```

## `Interval` — Date Intervals

Portable date interval that adapts per backend:

```rust
let i = Interval::days(30);
// SQLite:   30  (used with date functions)
// MySQL:    INTERVAL 30 DAY
// Postgres: INTERVAL '30 days'
```

See [`Interval`](vantage_sql::primitives::interval::Interval) for available constructors (`days`,
`hours`, `months`, etc.).

## `date_format()` — Date Formatting

Portable strftime-style formatting. Translates format tokens per backend:

```rust
let formatted = date_format(ident("created_at"), "%Y-%m-%d");
// SQLite:   STRFTIME('%Y-%m-%d', "created_at")
// MySQL:    DATE_FORMAT("created_at", '%Y-%m-%d')
// Postgres: TO_CHAR("created_at", 'YYYY-MM-DD')
```

[`date_format()`](vantage_sql::primitives::date_format::date_format) is a shorthand for
[`DateFormat::new()`](vantage_sql::primitives::date_format::DateFormat::new). Use
[`.raw_format()`](vantage_sql::primitives::date_format::DateFormat::raw_format) to skip token
translation and pass a native format string.
