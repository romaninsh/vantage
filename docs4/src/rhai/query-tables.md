# Tables Defined by a Query

A table usually reads one physical table or collection. A YAML spec for SQLite, PostgreSQL, MySQL
or SurrealDB can instead take its rows from a `SELECT` that a Rhai script builds. The script uses
the backend's query builder ([Query Builder Functions](./query-builders.md)), not the data
vocabulary, and it runs when the table is built, never when a data script runs.

<!-- toc -->

---

## A query as the source

`rhai:` in the driver block (`sqlite:`, `postgres:`, `mysql:` or `surreal:`) replaces `table:`.
The script's last expression must be a `select()`:

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

- The script runs once, when the Vista is built. A script that fails or doesn't end on a `select()`
  fails the build.
- The `SELECT` becomes a subquery in the table's `FROM`, aliased under the spec's name:
  `SELECT "id", "name" FROM (SELECT … FROM "product" WHERE price > 15) AS "expensive_products"`.
- `columns:` declares which of the query's output columns the table has, as for any table.
- Reads, conditions, sort, search and paging apply on top, as on any table. A data script can
  narrow `table("expensive_products")` with `where` and `sort` like any other table.
- Scalars the script passes to the builder (`expr("price > {}", [15])`, a comparison with a
  number) are bound as parameters.

## Deriving from another table

`base:` names another spec. It is resolved when the table is built, its `select()` is put in the
script's scope as `base`, and `inherit:` copies the columns and relations to keep. The script
transforms the base query instead of building one:

<!-- tested: vantage-sql tests/sqlite/6_vista.rs vista_yaml_base_aggregates_into_declared_column -->
```yaml
name: client_totals
id_column: client_id
columns:
  total_due: { type: int }
sqlite:
  base: orders
  inherit:
    columns: [client_id]
  rhai: |
    base.clear_fields().field("client_id").expression(expr("SUM(total) AS total_due")).group_by(expr("client_id"))
```

This is the aggregate pattern: drop the base's fields, group by a key, declare the aggregate as the
table's own column, and re-key the table with `id_column`. `clear_fields()` comes first because
PostgreSQL and MySQL reject non-grouped columns that SQLite tolerates. Without `rhai:`, `base:`
uses the base query as it is.

An inherited relation still works: following it from the derived table narrows the target by the
derived query, as in `client_id IN (SELECT "id" FROM (…) AS "debtors")`.

## Read-only

A table with `rhai:` or `base:` in its driver block is read-only: the factory turns off
`can_insert`, `can_update` and `can_delete`, because there is no single table a write could land
in. Callers find out the usual way, by checking capabilities. In Rhai, `insert`, `upsert`,
`patch` and a record's `save()` throw, and `delete` returns `false` (see
[Capabilities](./writes.md#capabilities)).

To keep a table writable while narrowing it with a native expression, use SurrealDB's `modify:`
instead, which changes a real table (see [`modify:` scripts](./surfaces.md#modify-scripts)).

## Arguments

A query-sourced table can take arguments. The host sets them on the driver block when it opens
the table; YAML can't set them. The script sees them as `args`, a map of strings, where `""` means
"not set". They reach `rhai:` sources and `base:` transforms, not `modify:` scripts.

<!-- tested: vantage-surrealdb vista::rhai_source::tests::args_map_is_in_scope -->
```rhai
let q = select().from("order");
if "bakery" in args && args.bakery != "" {
    q = q.where(expr("bakery.name = {}", [args.bakery]));
}
q
```

With `bakery` set to `"Breg"` the query gains the condition, with the name bound as a parameter.
Without it the script skips the condition instead of failing. Arguments change the query itself,
so they can filter rows before a `GROUP BY`, which a condition on the finished table can't.

Vantage UI, for example, sets them from a Scenery binding's `arg(name, value)` (see
[Layers](./layers.md#scenery-the-shape-of-a-view)).

## Why scripts can't run their own queries

Query building is allowed in one place: a table's source, written by whoever writes the YAML.
(SurrealDB's `modify:` scripts, `expr:` columns and reference scripts are configuration too, and
use the same builder.) A data script, such as an action body or an agent script, can only narrow
tables with `where`, `search` and `ref`. It can't send SQL, can't template a query, and actions
can't run custom queries.

That keeps every read inside the tables an application declared, every write inside the set rules
of [Safe writes](../record-lifecycle.md#safe-writes), and every value a data script passes a
parameter, never text spliced into a query. Allowing actions to run queries may come later, behind
its own permission.

[Config-Driven Vistas](../config-driven-vistas.md#the-rhai-layer) shows where these scripts sit
among the other YAML keys.
