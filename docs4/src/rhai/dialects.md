# Expression Dialects

Not every script in a Vantage configuration is a data script. Some build a backend's native query,
some compute one value from one row, and some are text with a few holes in it. They run on the same
`vantage-rhai` hosts with the same limits, but their vocabularies are separate from the data
vocabulary, and from each other. This chapter tells them apart and points to where each is
documented.

<!-- toc -->

---

## Data scripts and expression scripts

A data script works on sets of rows through Vistas: `table`, `where`, `list`, `insert`. It doesn't
know which backend it talks to.

An expression script builds a query in one backend's own terms: a SQL `SELECT`, a SurrealQL
condition, a graph path. It runs once, when a Vista is built, and its result becomes part of that
Vista's query. It reads no rows itself.

The two meet in one place: a backend's extension verbs on a data handle, such as SurrealDB's
`self.with_condition(expr)`, take an expression built with the backend's vocabulary (see
[Backend extensions](./surfaces.md#backend-extensions)).

## SQL query builders

A YAML spec for a SQL table can replace its source with a script-built `SELECT`. The script uses
`vantage-sql`'s builder (`select`, `from`, `field`, `where`, `group_by`, `expr`) and the shared
primitives (`count`, `avg`, `coalesce`, `case_when`, `date_format`, …):

<!-- tested: vantage-sql tests/sqlite/6_vista.rs vista_yaml_rhai_source_is_read_only_and_filters -->
```yaml
name: expensive_products
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
sqlite:
  rhai: |
    select().from("product").field("id").field("name").where(expr("price > 15"))
```

A query-sourced Vista is read-only. A derived Vista (`base:` plus `rhai:`) gets the base's
`select()` as `base` and transforms it. Both are described in
[Config-Driven Vistas](../config-driven-vistas.md#the-rhai-layer), and the primitives in
[SQL Primitives](../sql/primitives.md).

`where` here is the SQL builder's method and takes an expression. On a data handle, `where` takes a
column and a value. The name is the same because both narrow, but the two vocabularies never share
a host.

## SurrealDB expressions

`vantage-surrealdb` registers the same builder shape for SurrealQL, the same primitive names where
the concept exists in both, and SurrealDB's own terms where it doesn't (graph paths, `SPLIT`,
`me`). It backs `rhai:` sources, `base:` transforms, and the expressions handed to
`with_condition`. See [SurrealDB Primitives](../surrealdb/primitives.md).

The SQL and SurrealDB builders are two copies of a similar grammar. Merging them is a known
follow-up and not part of the data vocabulary.

A SurrealDB column spec may also carry `expr:`, a server-side computed column: the script is
evaluated once with this vocabulary, and the backend projects the result as `(<expr>) AS <column>`
on every read. Unlike a `lazy:` column it is part of the query, so it can follow record links and
call SurrealDB functions. Other backends reject or ignore it.

## `lazy:` columns

A column's `lazy:` script is a third kind: it computes one value from one row, in the Vista, after
the backend has returned the row. It uses neither the data vocabulary nor a query builder, only
`row`. [Computed columns](./computed.md) covers it.

## Templates

Many YAML strings are templates: text with `${ … }` holes. `vantage-rhai`'s
[`Template`](vantage_rhai::Template) slot evaluates each hole as a Rhai expression against the
host's scope and joins the results. A template that is exactly one hole (surrounded by whitespace
at most) keeps the hole's value with its type, so `"${ row.total }"` stays a number and a map stays
a map.

Two look-alikes are not Rhai:

- `import_from`'s mapping (see [Writes](./writes.md#importing)) only understands
  `${row.<col>}` column references. Other holes stay in the text as written.
- Vantage UI's action texts (`title:`, `description:`, `terminal.args`) substitute `${row.x}` and
  `${form.x}` paths, with no expressions.

## Command scripts

`vantage-cmd` tables read by running a Rhai script that calls `run(...)` for a subprocess and
`parse_json` / `parse_jsonl` on its output, then returns an array of row maps. The script sees the
read's conditions, columns, limit and offset as variables. It is a third dialect, for shaping
command output into rows, and is documented in the `vantage-cmd` crate.
