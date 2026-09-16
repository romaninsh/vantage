# vantage-api-client Architecture

Two adapters, one crate: `RestApi` turns a `Table<RestApi, E>` into
`GET {base_url}/{endpoint}?…`, and `GraphqlApi` turns a `Table<GraphqlApi, E>`
into a rendered query document posted to one endpoint. Both implement
`vantage-table`'s `TableSource`, both bridge into `vantage-vista`'s `Vista`
through a factory, and both send through one `ResilientClient` per API.
This document is for maintainers and adapter authors; the reader guides
are linked from the [README](README.md).

## Layer diagram

```
Vista (vantage-vista)                  what UIs and Diorama consume
   │  RestApiVistaFactory / GraphqlApiVistaFactory     vista/factory.rs, vista/spec.rs
   │  RestApiTableShell / GraphqlApiTableShell          vista/source.rs
   ▼
Table<RestApi, E> / Table<GraphqlApi, E>  (vantage-table)
   │  TableSource impl                                  rest/table_source.rs, graphql/impls/table_source.rs
   ▼
RestApi                      GraphqlApi
   endpoint templates           GraphqlSelect builder + renderer   graphql/select/
   query string / client        GraphqlCondition + FilterDialect    graphql/condition.rs, operation.rs
     filters, pagination        typed scalars (AnyGraphqlType)      graphql/types/
   response shapes              response_path, root_args
   │  fetch_raw_body              │  post_graphql
   ▼                              ▼
transport.rs        ClientConfig · build_client · policy_for · client_error
   ▼
ResilientClient (vantage-api-pool)   cap · rate limit · breaker · retry · observer
   ▼
reqwest
```

## Request lifecycle: REST

`RestApi::fetch_raw_body(table_name, window, conditions)` is the single
request site for REST.

1. **Resolve deferred conditions.** A `with_one` traversal may carry a
   `DeferredFn` that fetches the parent row at request time; every
   condition is resolved to a plain scalar first.
2. **Endpoint.** `table_name` is a path under `base_url`, optionally with
   `{placeholder}` segments (`users/{userId}/albums`). Each placeholder is
   filled from the eq-condition whose field has that name; those conditions
   are consumed. A missing one is an error.
3. **Filters.** Under `FilterStrategy::Query` the remaining eq-conditions
   become `?field=value`. Under `FilterStrategy::Client` none go on the
   wire; they are kept as `(field, value)` pairs and applied to the parsed
   rows, which is for APIs that reject or ignore unknown query params.
4. **Pagination.** A window `(offset, limit)` is appended as
   `?_page=N&_limit=M` (1-based page) or `?skip=N&limit=M` (0-based offset)
   per `PaginationParams`. `no_pagination` short-circuits any page after the
   first to an empty result.
5. **Send.** `policy_for(Priority::current())` picks `essential()` or
   `background()`; `execute_with` builds `GET url` with the auth header on
   every attempt. Failures become `VantageError { "API request failed",
   url, attempts, kind, status?, body? }` via `client_error`.
6. **Parse.** The body is read as JSON; `ResponseShape` locates the row
   array: the body itself (`BareArray`), `body[array_key]` (`Wrapped`), or
   `body[table_name]` (`WrappedByTableName`). `total_key`, when configured,
   is read from the envelope. Each row becomes a `Record<CborValue>` keyed
   by the id field (or its index). Client filters are applied. `RowsPulled {
   n }` is reported with the final count.

`fetch_total` sends a `(0, 1)` window and reads only `total_key`; it is the
count probe the paged grid uses to size its scrollbar. `preview_request`
renders the same URL without sending, and masks the auth header.

Capabilities advertised to Vista: `can_count`, `can_traverse_to_record`,
and `can_fetch_window` only when `total_key` is set. No server-side sort,
search or operators: a REST Vista sorts and searches what it loaded. REST
is read-only at the `TableSource` level; writes go through
`RestApi::http_request`.

## Request lifecycle: GraphQL

`GraphqlApi::post_graphql(query, variables)` is the single request site.

1. **Build.** `GraphqlSelect` collects the root field, the selected fields
   (nested objects render as `rocket { rocket_name }`), the condition tree,
   ordering and pagination. `root_args` (literal arguments from YAML or the
   builder) render before the filter.
2. **Render.** The filter renders inline as GraphQL value syntax under the
   dialect's argument name (`where` for Hasura, `find` or `filter_arg` for
   Generic) so no input-type name is needed; pagination goes through typed
   variables `$limit: Int`, `$offset: Int`. `preview` is the synchronous
   twin used by `preview_query`.
3. **Send.** Same policy selection as REST; `POST endpoint` with
   `{ query, variables }` and the auth header, rebuilt per attempt. A
   non-2xx becomes `VantageError { "GraphQL request failed", endpoint,
   attempts, kind, status?, body? }`.
4. **Parse.** A `2xx` whose envelope has a non-empty `errors` array is a
   logical error: `GraphQL response carried errors` with the joined
   messages. It happens after the transport succeeded, so it never retries
   and never counts against the breaker. Otherwise `data[root]`, descended
   through `response_path`, is the row array (or a single object for
   get-by-id). `RowsPulled` is reported after decoding.

Capabilities: `can_count`, `can_traverse_to_record`, and `can_order`,
`can_search`, `can_filter_operators`, `can_set_page_size` according to the
`Supports` flags (all true by default). A YAML `graphql:` block can turn
any of them off for endpoints that ignore the argument, so the consumer
sorts or filters locally instead of sending something the server drops.
GraphQL is read-only.

### Dialects

`FilterDialect::Hasura` renders `{ field: { _eq: v } }` with `_and`, `_or`,
`_not` and the full `GraphqlOp` set (`Eq Ne Gt Gte Lt Lte In NotIn Like
ILike IsNull IsNotNull`). `FilterDialect::Generic` renders `{ field: v }`,
equality only; any other operator, `OR` or `NOT` fails at render time with
an error naming the dialect. `add_op_condition` reports `Like` as
`Unimplemented` so a Vista consumer evaluates pattern filters locally.

### Types

`AnyGraphqlType` wraps `serde_json::Value` with a scalar variant
(`Int`, `BigInt`, `Float`, `String`, `Id`, `DateTime`, `Date`, `Time`,
`Uuid`, `Decimal`, `Json`, `Object`, `Array`, `Bool`, `Null`) so a value's
meaning survives the JSON round trip; `GraphqlType` maps Rust types onto
variants, and `vantage_type_system!` lets a downstream crate widen the
variant set.

## Traversal

Both adapters implement `related_in_condition` for `with_many` /
`with_one`:

- **Peel.** When the parent table carries an eq-condition on the join
  field (`users` narrowed to `id = 1`), the child gets that value directly:
  REST substitutes it into a template or adds a query param; GraphQL adds
  an eq filter.
- **Defer.** Otherwise the child gets a `DeferredFn` that fetches the parent
  row at request time and reads the foreign key from it. REST resolves
  deferreds at the top of `fetch_raw_body`; GraphQL at render time.

GraphQL relations are two round trips today; single-document nested
selection is on the roadmap.

## Vista bridge and YAML

`RestApiVistaSpec` / `GraphqlApiVistaSpec` are `vantage-vista` specs with
adapter extras. REST: `api: { endpoint }` per table (defaults to the table
name). GraphQL: `graphql: { root_field, dialect, filter_arg, args,
response_path, filter, order, search, paginate }` per table and `graphql: {
field }` per column. The factories build a `Vista` whose `driver()` is
`"rest-api"` or `"graphql"`; `RestApiVistaFactory::register_yaml` keeps a
registry so cross-model references resolve by name.

## Transport glue

`transport.rs` is the only file that knows both this crate and
`vantage-api-pool`:

- `ClientConfig { max_parallel: 4, rate_limit, observer, http }` is what
  both builders collect; `build_client` turns it into a `ResilientClient`
  with `default_breaker()`.
- `policy_for(Priority)` maps `Essential` → `CallPolicy::essential()`,
  everything else → `background()`.
- `client_error(ClientError, what, url)` produces the `VantageError` shape
  above, keeping the URL and body in the context and out of the message.
- `AuthHeader` is the newtype behind `auth_header` whose `Debug` prints
  `<set>`; every `Debug` derive in the crate goes through it so a token
  can never reach a log.

The behaviour of the client itself, including the breaker rules and the
observer contract, is documented in
[vantage-api-pool/ARCHITECTURE.md](../vantage-api-pool/ARCHITECTURE.md);
the datasource-side view is [README_transport.md](README_transport.md).

## Errors

| Message | Where | Context keys |
|---|---|---|
| `API request failed` | REST transport/status | `url`, `attempts`, `kind`, `status`, `body` or `detail` |
| `GraphQL request failed` | GraphQL transport/status | `endpoint`, `attempts`, `kind`, `status`, `body` or `detail` |
| `GraphQL response carried errors` | logical errors in a 2xx | `errors` (joined messages) |
| `Failed to parse API response as JSON` | body not JSON | `detail` |
| `Response missing array under wrapper key` | `ResponseShape` mismatch | `key` |
| `Expected response body to be a JSON array (BareArray shape)` | envelope where an array was expected | |
| `total_key missing or not an integer in API response` | count probe | `total_key` |
| `No eq-condition provided for URI placeholder` | template without its condition | `placeholder`, `table_name` |
| `REST API is a read-only data source` | any write through `TableSource` | |

`VantageError::Display` prints the message followed by `(key: value, …)`,
so status codes and server messages are greppable in logs and testable
with `to_string().contains(..)`.

## Concurrency

Nothing in this crate spawns. A request runs on the caller's task; cancel
the caller and the request, its retries and its permit are released. The
`ResilientClient` inside each `RestApi`/`GraphqlApi` is shared by every
clone of that API, so the parallel cap, rate limit, breaker and observer
key are per API instance: build one per remote API and clone it per table,
never one per table.

## Tests

- `tests/rest_transport.rs`, `tests/graphql_transport.rs`, with
  `tests/support/mod.rs`: wiremock contract tests for the transport
  behaviour (priority, retries, breaker, errors, observer, writes, custom
  client). Nothing touches the network.
- `tests/builder.rs`, `tests/yaml_factory.rs`: builder defaults and YAML
  spec parsing.
- `src/**` unit tests: query-string rendering, templates, dialect
  rendering, type mapping. One `#[ignore]`d test hits the live Launch
  Library dev host.
