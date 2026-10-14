# Connecting to a GraphQL API

This guide takes a GraphQL endpoint from "I have a URL" to typed tables
whose conditions render as the server's filter syntax, with pagination,
traversal and honest errors. The one decision that shapes everything is the
dialect: how the server spells a filter.

## 1. Find the dialect

Look at the schema, or at what a working query sends:

| The server accepts | Dialect | Filter argument |
|---|---|---|
| `launches(where: { mission_name: { _eq: "X" } })`, with `_and`/`_or`/`_not` | `FilterDialect::Hasura` | `where` |
| `launches(find: { mission_name: "X" })`, equality only | `FilterDialect::Generic` | `find` (change with `filter_arg_name`) |

Generic covers most hand-written schemas: one flat argument object, only
equality. Asking it for `>`, `LIKE`, `OR` or `NOT` fails at render time with
an error that names the dialect, rather than sending something the server
would reject or ignore.

## 2. Build the API

```rust
use vantage_api_client::{FilterDialect, GraphqlApi};

let api = GraphqlApi::builder("https://spacex-api.fly.dev/graphql")
    .dialect(FilterDialect::Generic)
    .filter_arg_name("find")
    .auth("Bearer …")
    .max_parallel(4)
    .build();
```

One `GraphqlApi` per endpoint, cloned into every table: the parallel cap,
rate limit, breaker and health reporting are per API instance.

Other knobs:

- `.root_args(json!({ "input": {} }))` renders literal arguments on the root
  field before the filter, for schemas that require one.
- `.response_path("edges.node")` descends into the result before reading
  rows, for connection-style responses.
- `.supports(Supports::default().with_order(false))` (`use
  vantage_api_client::Supports`) tells the adapter what the endpoint
  honours; anything switched off is reported as unavailable so a Vista
  consumer sorts, searches or filters locally instead of sending an
  argument the server drops. Defaults: filter on; order and search on for
  Hasura only; paginate off; count by listing.
- `.count_field("launches_agg")` names the Hasura aggregate field used by
  `CountMode::Aggregate` (default `<root_field>_aggregate`).
- `.rate_limit(..)`, `.observer(..)`, `.request_timeout(..)`,
  `.http_client(reqwest::Client)`: see
  [README_transport.md](README_transport.md).

## 3. Declare tables and conditions

```rust
use vantage_api_client::{GraphqlApi, GraphqlOperation};
use vantage_table::{column::core::Column, table::Table};
use vantage_types::EmptyEntity;

let mut launches = Table::<GraphqlApi, EmptyEntity>::new("launches", api.clone())
    .with_id_column("id");
launches.add_column(Column::<String>::new("mission_name"));
launches.add_column(Column::<String>::new("launch_year"));
launches.add_condition(Column::<String>::new("launch_year").eq("2018"));

let rows = launches.list_values().await?;
// POST { query: "query { launches(find: {launch_year: \"2018\"}) { id mission_name launch_year } }" }
```

The table name is the root field. Selected columns are the fields
requested; a dotted column (`rocket.rocket_name`) renders as a nested
selection. `GraphqlOperation` gives every typed column `.eq() .ne() .gt()
.gte() .lt() .lte() .in_() .like() .ilike() .is_null()`; the condition is
kept abstract and rendered at request time in the dialect's spelling, so
the same code targets Hasura and a Generic schema.

The filter renders inline as GraphQL value syntax, so no schema lookup is
needed to type it. Field names, argument names and filter keys are checked
against the GraphQL name grammar before anything is sent.

### Paging

Paging is opt-in: a schema whose list field takes no `limit`/`offset` would
reject every query. Turn it on with `Supports::default().with_paginate(true)`
(or `paginate: true` in YAML). The page then goes through the typed
variables `$limit` and `$offset`:

```text
query ($limit: Int, $offset: Int) { launches(find: {…}, limit: $limit, offset: $offset) { … } }
```

A Vista offers `fetch_page`, `fetch_window` and `fetch_next` only when both
`paginate` and `filter` are on. With `filter: false` the conditions run on
the fetched rows, which would cut a server page short, so the table is read
whole instead.

Offset paging is stable only under a fixed order. On Hasura a paged read
with no order of its own is ordered by the id column. A Generic schema has
no `order_by`, so stable pages depend on the server returning rows in a
consistent order; rows inserted between two page reads shift the pages.

Rows are keyed by their id. A row with a null or empty id is keyed
`~row:<position>` (its absolute position in the result), and two rows with
the same id in one response are an error rather than one silently replacing
the other.

### Ordering and search

Both render on Hasura only (`order_by:`, `_ilike`), unless `Supports` says
otherwise. A Vista sort on a dotted column is refused. A quicksearch ORs
`_ilike` over the columns flagged `searchable`; with none flagged, over the
string-typed columns that are not the id. Dotted columns are skipped, and a
table with no qualifying column matches nothing. `%`, `_` and `\` in the
search text are escaped. A Vista `contains` filter (`FilterOp::Like`)
renders as `_ilike: "%…%"` on Hasura, escaped the same way, and is refused
on Generic.

## 4. Traversal

```rust
let rockets = Table::<GraphqlApi, EmptyEntity>::new("rockets", api.clone())
    .with_id_column("id")
    .with_many("launches", "rocket_id", |api| Table::new("launches", api));
```

A parent eq-condition on the join field is peeled into a child filter
(`launches(find: { rocket_id: "falcon9" })`); otherwise the child carries a
deferred lookup that reads the key from the parent row at request time.
Relations are two round trips; the join field must be one the server's
filter input accepts, which for a Generic schema means introspecting the
`find` input type and using exactly its keys.

## 5. From YAML

```yaml
name: launches
id_column: id
columns:
  id: { type: string, flags: [id] }
  mission_name: { type: string, flags: [title] }
  launch_year: { type: string }
  rocket.rocket_name: { type: string }
graphql:
  root_field: launches
  dialect: generic
  filter_arg: find
  order: false        # the server ignores ordering; sort locally
  search: false
  paginate: true      # sends limit/offset; enables fetch_page/fetch_window/fetch_next
                      # (not offered with filter: false)
```

A `graphql:` block overrides the API's `Supports` for that table key by key:
a key the block leaves out keeps the builder's setting.

### Counting

`count: list | aggregate`, default `list`. With `list`, `get_count` lists
the rows and counts them; this works for every role. On a Hasura table,
`count: aggregate` runs `<root_field>_aggregate(where: …) { aggregate { count } }`
instead. It needs a role with `allow_aggregations`; a server that rejects
the query fails the count with an error that suggests `count: list`, and
nothing falls back silently. The Generic dialect and `filter: false` tables
always count by listing. Set `count_field:` when the aggregate field is
named differently. Both paths apply the same conditions, search included.

```rust
use vantage_api_client::{GraphqlApi, GraphqlApiVistaSpec};
use vantage_vista::VistaFactory;

let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(YAML)?;
let launches = api.vista_factory().build_from_spec(spec)?;
```

Column types map to typed columns: `int`, `bigint`, `float`, `bool`,
`string`, `datetime`, `date`, `time`, `uuid`, `json`; an unknown type fails
at build time. `dialect:` takes `generic` or `hasura`; any other value fails
to parse. A column may carry `graphql: { field: mission_name }` when the
server field is named differently: the query selects it as
`column: mission_name`, so rows keep the column name, and filters, ordering
and search on the column use the server field. Both names must be plain
(undotted). The `searchable` flag picks the quicksearch columns. The
`examples/schema/*.yaml` files describe ten SpaceX entities this way.

### References

```yaml
name: launches
columns:
  id: { type: string, flags: [id] }
  rocket_id: { type: string }
references:
  rocket: { table: rockets, kind: has_one, foreign_key: rocket_id }
```

`has_one` reads the parent row's `foreign_key` and narrows the target by
its id; `has_many` narrows the target's `foreign_key` column by the parent's
id. Either way the target gets an eq condition: sent to the server with
filter push-down on, applied to the fetched rows with `filter: false`. A key
held as text is sent as an integer when the target column is an int.
Multi-column `keys:` are refused at build time.

The target is built from its own spec, on the factory's API, never with the
parent table's `graphql:` overrides. Specs come from one of two places:

```rust
let mut factory = api.vista_factory();
factory.register_yaml(LAUNCHES_YAML)?;
factory.register_yaml(ROCKETS_YAML)?;
let launches = factory.build("launches")?;
```

or, when the specs live elsewhere, a resolver that returns a spec by name:

```rust
use std::sync::Arc;
use vantage_api_client::GraphqlApiVistaSpec;
use vantage_vista::SpecResolver;

let resolver: Arc<dyn SpecResolver<GraphqlApiVistaSpec>> =
    Arc::new(move |name: &str| specs.get(name).cloned());
let factory = api.vista_factory().with_resolver(resolver);
```

A reference whose target has no spec fails with an error naming the
relation and the target; nothing is sent.

## 6. Errors and retries

Three distinct failures, each a `VantageError`:

- **Transport or HTTP status** — `GraphQL request failed` with `url`,
  `attempts`, `kind`, `status` and up to 500 characters of `body`. A `400`
  from a schema that rejects an argument shows its message here. Retried
  only when someone is waiting (`Priority::Essential.scope(..)`) and only
  for `5xx`/transport failures; a `4xx` is final.
- **Logical errors** — a `2xx` whose envelope carries `errors`:
  `GraphQL response carried errors` with the messages joined. Never
  retried, never counted against the breaker; the transport succeeded.
- **Render errors** — an operator or `OR`/`NOT` the dialect cannot spell,
  or a field, argument or filter key that is not a valid GraphQL name
  (`GraphQL name is not valid`), raised before anything is sent. A YAML
  spec with a bad name fails when the Vista is built.

The retry, breaker and rate-limit rules are in
[README_transport.md](README_transport.md).

## 7. Seeing the document

`Vista::preview_query()` (the Vantage app's `preview_query` tool) renders
the query document synchronously, plus the endpoint. It matches what
`post_graphql` sends, except that `limit` and `offset` are inlined instead
of passed as variables, so the document reads on its own. When a filter seems
ignored, read the rendered argument: a Generic dialect only ever renders
equality, and a field the server's input type does not know is sent and
silently dropped by most servers.

## 8. Writes

GraphQL tables are read-only: insert, replace, patch and delete return an
`Unsupported` error, as do `sum`, `min` and `max`. For a mutation, post the
document yourself through the shared client:

```rust
let data = api.post_graphql(
    "mutation($id: ID!) { launchMission(id: $id) { id } }",
    &variables,
).await?;
```

It shares the API's cap, breaker and health, and follows the same retry
rules; a mutation retried under `Essential` must be idempotent on the
server's side.

## Worked example

`cargo run --example graphql_spacex -- launches mission_name=FalconSat`
drives the ten SpaceX entities from `examples/schema/*.yaml` against the
public mirror, and the Vantage app's `spacex` example app is the same
schema as a YAML inventory with relations and aggregates.
