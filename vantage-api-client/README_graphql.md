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
- `.supports(Supports { order: Some(false), .. })` tells the adapter what
  the endpoint honours; anything switched off is reported as unavailable
  so a Vista consumer sorts, searches or filters locally instead of sending
  an argument the server drops.
- `.rate_limit(..)`, `.observer(..)`, `.client(reqwest::Client)` (alias
  `http_client`): see [README_transport.md](README_transport.md).

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
// POST { query: "query($limit: Int, $offset: Int) { launches(find: {launch_year: \"2018\"}, limit: $limit, offset: $offset) { id mission_name launch_year } }" }
```

The table name is the root field. Selected columns are the fields
requested; a dotted column (`rocket.rocket_name`) renders as a nested
selection. `GraphqlOperation` gives every typed column `.eq() .ne() .gt()
.gte() .lt() .lte() .in_() .like() .ilike() .is_null()`; the condition is
kept abstract and rendered at request time in the dialect's spelling, so
the same code targets Hasura and a Generic schema.

Pagination goes through typed variables `$limit` and `$offset`, which every
server accepts without a schema lookup; the filter renders inline as
GraphQL value syntax for the same reason.

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
```

```rust
use vantage_api_client::{GraphqlApi, GraphqlApiVistaSpec};
use vantage_vista::VistaFactory;

let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(YAML)?;
let launches = api.vista_factory().build_from_spec(spec)?;
```

Column types map to typed columns: `int`, `bigint`, `float`, `bool`,
`string`, `datetime`, `date`, `time`, `uuid`, `json`; an unknown type fails
at build time. A column may carry `graphql: { field: … }` when the wire
name differs. The `examples/schema/*.yaml` files describe ten SpaceX
entities this way.

## 6. Errors and retries

Three distinct failures, each a `VantageError`:

- **Transport or HTTP status** — `GraphQL request failed` with `endpoint`,
  `attempts`, `kind`, `status` and up to 500 characters of `body`. A `400`
  from a schema that rejects an argument shows its message here. Retried
  only when someone is waiting (`Priority::Essential.scope(..)`) and only
  for `5xx`/transport failures; a `4xx` is final.
- **Logical errors** — a `2xx` whose envelope carries `errors`:
  `GraphQL response carried errors` with the messages joined. Never
  retried, never counted against the breaker; the transport succeeded.
- **Render errors** — an operator or `OR`/`NOT` the dialect cannot spell,
  raised before anything is sent.

The retry, breaker and rate-limit rules are in
[README_transport.md](README_transport.md).

## 7. Seeing the document

`Vista::preview_query()` (the Vantage app's `preview_query` tool) renders
the query document synchronously with the filter inlined, exactly as
`post_graphql` would send it, plus the endpoint. When a filter seems
ignored, read the rendered argument: a Generic dialect only ever renders
equality, and a field the server's input type does not know is sent and
silently dropped by most servers.

## 8. Writes

GraphQL tables are read-only; mutations are not rendered. For a mutation,
post the document yourself through the shared client:

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
