# Query Builder Functions

This page lists every function the SQL and SurrealDB query builders register for Rhai. They are
available where a script builds a native query: a [query-sourced table](./query-tables.md)'s
`rhai:` and `base:` scripts, and on SurrealDB also `modify:` scripts, `expr:` columns and
reference build scripts. Data scripts don't get them (see
[Why scripts can't run their own queries](./query-tables.md#why-scripts-cant-run-their-own-queries)).
The data vocabulary is listed in [Words at a glance](../rhai.md#words-at-a-glance).

SQLite, PostgreSQL and MySQL share one registration (`vantage_sql::register_engine!`), so the
"SQL" column holds for all three; where the output differs per database, the row says so.
SurrealDB registers its own copy (`vantage_surrealdb::rhai_engine::register_surreal_onto`) with
the same names where the concept exists in both. The Rust side of each primitive is in
[SQL Primitives](../sql/primitives.md) and [SurrealDB Primitives](../surrealdb/primitives.md).

In the signatures, `x` is an expression or an identifier, `sel` a `select()`, and `"…"` a string.
Where a function takes a plain value as well, it says so.

<!-- toc -->

---

## Values in a query

Two rules hold in both builders:

- A number, bool or string passed as an argument (to `expr`'s array, to a comparison, to
  `coalesce` and `thing` on SurrealDB) becomes a bound parameter, never query text.
- A string passed as a name (`ident`, `from`, `field`, `alias`) or as a template (`expr`,
  `cast`'s type) is written into the query. These come from the configuration's author,
  not from data.

## Constructors

| Function | SQL | SurrealDB | Builds |
|---|---|---|---|
| `select()` | yes | yes | an empty `SELECT` |
| `ident("name")` | yes | yes | a quoted identifier |
| `table("name")` | yes | yes | the same as `ident`; on a host that also has the data vocabulary, `table(name)` is the data verb |
| `expr("text")` | yes | yes | raw query text |
| `expr("a = {}", [args])` | yes | yes | text with `{}` holes; each argument is an expression, an identifier, or a value bound as a parameter |
| `fx("name", [x, …])` | yes | yes | a function call `name(x, …)`; arguments must be expressions or identifiers |
| `case_when()` | yes | yes | an empty conditional (see [Conditionals](#conditionals)) |
| `window()` | yes | — | an empty window spec (see [Window functions](#window-functions)) |
| `thing("table", "id")` | — | yes | a record id, `type::record(…)` with both parts bound as parameters |
| `param("name")` | — | yes | a parameter, `$name` |
| `parent()`, `parent("field")` | — | yes | `$parent`, `$parent.field` |
| `time_now()` | — | yes | `time::now()` |
| `me` (a variable) | — | yes | the current record, the anchor of a graph path |

## Identifiers and expressions

| Function | SQL | SurrealDB | Builds |
|---|---|---|---|
| `id.dot_of("s")` | yes | yes | **differs**: SQL puts `s` in front (`ident("name").dot_of("u")` is `"u"."name"`); SurrealDB puts it after (`ident("t").dot_of("f")` is `t.f`) |
| `id["col"]` | yes | yes | SQL: the column `col` qualified by the identifier's alias or name; SurrealDB: the path `id.col` |
| `x["field"]`, `x[n]` | — | yes | a field of an expression, or element `n` of an array |
| `id.alias("a")` | yes | yes | SQL: the identifier with an alias, rendered `"users" AS "a"`; SurrealDB: the expression `id AS a` |
| `x.alias("a")` | yes | yes | `x AS a`, with `a` quoted as an identifier |
| `x.clone()` | yes | yes | a copy |

## Select methods

Each method returns the select, so they chain.

| Method | SQL | SurrealDB | Adds |
|---|---|---|---|
| `from("table")`, `from(id)` | yes | yes | a source |
| `from(x)` | — | yes | an expression as the source, such as a graph path or a record id |
| `from_as("table", "alias")` | yes | — | an aliased source |
| `from_as(sel, "alias")` | yes | — | a derived table, `FROM (SELECT …) AS "alias"` |
| `field("name")` | yes | yes | a column |
| `clear_fields()` | yes | yes | drops every field so far; call it before grouping a `base` |
| `expression(x)` | yes | yes | an expression as a field; name it with `.alias("a")` |
| `where(x)` | yes | yes | a condition; several are joined with `AND` |
| `group_by(x)` | yes | yes | a `GROUP BY` term |
| `order_by(x, "asc"\|"desc")` | yes | yes | an `ORDER BY` term; any other direction is an error |
| `having(x)` | yes | — | a `HAVING` condition |
| `distinct()` | yes | yes | `DISTINCT` |
| `limit(n, skip)` | yes | yes | `LIMIT` and offset; SurrealDB writes `LIMIT n START skip` and leaves out `START 0` |
| `inner_join("table", "alias", on)` | yes | — | `INNER JOIN` |
| `left_join("table", "alias", on)` | yes | — | `LEFT JOIN` |
| `group_all()` | — | yes | `GROUP ALL`: the whole result as one aggregate row |
| `split("field")`, `split(id)` | — | yes | `SPLIT`: one row per element of an array field |
| `only()` | — | yes | `SELECT ONLY`: a single record |
| `value()` | — | yes | `SELECT VALUE`: plain values instead of objects |
| `sel.subquery()` | — | yes | the select as an expression, `(SELECT …)` |

## Aggregates

| Function | SQL | SurrealDB |
|---|---|---|
| `count(x)` | `COUNT(x)` | `count(x)` |
| `count()` | — | `count()` |
| `count_distinct(x)` | `COUNT(DISTINCT x)` | `count(array::distinct(x))` |
| `sum(x)` | `SUM(x)` | `math::sum(x)` |
| `avg(x)` | `AVG(x)` | `math::mean(x)` |
| `min(x)`, `max(x)` | `MIN(x)`, `MAX(x)` | `math::min(x)`, `math::max(x)` |
| `group_concat(x, distinct)` | SQLite `GROUP_CONCAT(x, ',')`, PostgreSQL `STRING_AGG(x, ',')`, MySQL `GROUP_CONCAT(x SEPARATOR ',')`; `distinct` is a bool | — |
| `median(x)`, `stddev(x)` | — | `math::median(x)`, `math::stddev(x)` |
| `array_group(x)` | — | `array::group(x)` |

## Scalar functions

| Function | SQL | SurrealDB |
|---|---|---|
| `coalesce(a, b)` | `COALESCE(a, b)`; `a` and `b` must be expressions or identifiers | `a ?? b`; values allowed |
| `nullif(a, b)` | `NULLIF(a, b)`; expressions or identifiers | `IF a = b THEN NONE ELSE a END`; values allowed |
| `cast(x, "type")` | `CAST(x AS type)` | `type::<type>(x)` |
| `round(x, n)` | `ROUND(x, n)` | `math::fixed(x, n)` |
| `round(x)` | — | `math::round(x)` |
| `date_format(x, "%Y-%m")` | SQLite `STRFTIME`, MySQL `DATE_FORMAT`, PostgreSQL `TO_CHAR` with the tokens translated (`%Y %m %d %H %M %S`) | `time::format(x, "fmt")` |
| `time_group(x, "unit")` | — | `time::group(x, unit)`; the unit is bound |
| `lower(x)` | — | `string::lowercase(x)` |
| `words(x)` | — | `string::words(x)` |
| `string_len(x)` | — | `string::len(x)` |
| `similarity(x, "term")` | — | `string::similarity::jaro_winkler(x, term)`; the term is bound |
| `first(x)` | — | `array::first(x)` |
| `len(x)` | — | `array::len(x)` |
| `object_entries(x)`, `object_values(x)` | — | `object::entries(x)`, `object::values(x)` |
| `type_float(x)`, `type_int(x)` | — | `type::float(x)`, `type::int(x)` |

## Arithmetic and comparison

| Function or operator | SQL | SurrealDB | Builds |
|---|---|---|---|
| `add(a, b)`, `sub(a, b)`, `mul(a, b)`, `div(a, b)` | yes | yes | `(a + b)` and so on; expressions or identifiers only |
| `a + b`, `a - b`, `a * b`, `a / b` | — | yes | the same, where one side is an expression and the other an expression or a number |
| `a == b`, `a != b`, `a < b`, `a > b`, `a <= b`, `a >= b` | yes | yes | a condition; `==` is written `=`. Either side may be an expression, an identifier or a value; SurrealDB also takes an array `[…]` or a map `#{…}` literal |

## Conditionals

`case_when()` starts a conditional, `.when(cond, then)` adds a branch, `.else_(value)` sets the
fallback, and `.expr()` turns it into an expression:

| Method | SQL | SurrealDB |
|---|---|---|
| `.when(cond, then)` | `cond` an expression or identifier, `then` an expression | either may be a value |
| `.else_(value)` | an expression or identifier | any value |
| `.expr()` | `CASE WHEN … THEN … ELSE … END` | `IF … THEN … ELSE … END` |

## Window functions

SQL only. `window()` starts a window spec; each method returns it.

| Method | Adds |
|---|---|
| `.partition_by(x)` | a `PARTITION BY` term |
| `.order_by(x, "asc"\|"desc")` | an `ORDER BY` term |
| `.rows("from", "to")` | `ROWS BETWEEN from AND to`, such as `"UNBOUNDED PRECEDING"`, `"CURRENT ROW"` |
| `.range("from", "to")` | `RANGE BETWEEN from AND to` |
| `.apply(x)` | the expression `x OVER (…)`, such as `window().partition_by(…).apply(sum(…))` |

## Graph paths

SurrealDB only.

| Function | Builds |
|---|---|
| `graph(me, "edge", "table", …)` | an outward path `->edge->table…` from the anchor; 2 to 7 arguments |
| `graph("table", "edge", …, me)` | an inward path `table<-edge<-…`; the anchor's position sets the direction |
| `graph(…, graph(…), …)` | a nested path; the anchor may be a sub-path, which changes direction per hop |
| `recurse(path, min, max)` | `@.{min..max}(path)` |
| `arrow(id, "edge")` | `id->edge` |
| `back(id, "edge")` | `id<-edge` |
| `arrow_field(id, "edge", "field")` | `id->edge.field` |

Exactly one argument of `graph` is the anchor (`me` or a sub-path); it must be first or last.

## Array closures

SurrealDB only. These take native Rhai closures and build SurrealQL from them; the closure's
parameters become engine-named placeholders (`$value`, `$acc`).

| Method | Builds |
|---|---|
| `x.map(\|v\| body)` | `x.map(\|$value\| body)` |
| `x.filter(\|v\| body)` | `x.filter(\|$value\| body)` |
| `x.fold(init, \|acc, v\| body)` | `x.fold(init, \|$acc, $value\| body)` |

## On a table handle

SurrealDB adds one verb to the data vocabulary's handles: `self.with_condition(x)` narrows the
table by a native expression. It works on `self` only (see
[Backend extensions](./surfaces.md#backend-extensions)).
