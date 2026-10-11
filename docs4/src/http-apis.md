# HTTP APIs: REST & GraphQL

The `vantage-api-client` crate reads tables from HTTP APIs. `RestApi` turns a table into
`GET {base_url}/{endpoint}?…`, and `GraphqlApi` turns it into one query document posted to an
endpoint. Both send through `vantage-api-pool`'s `ResilientClient`, which caps parallel requests,
retries, rate-limits and trips a circuit breaker per API.

Both drivers read a [YAML spec](./config-driven-vistas.md) through their Vista factory, the same
way the database drivers do. The universal keys (`name`, `id_column`, `columns`, `references`)
work as everywhere else. Each driver adds one table-level block: `api:` for REST and `graphql:`
for GraphQL. GraphQL also has a column-level `graphql:` block.

Settings that belong to the endpoint as a whole (base URL, auth, response envelope, paging
parameter names, timeouts) are set once on the `RestApi` / `GraphqlApi` builder. Every table
built from that API shares them. The YAML blocks carry only what differs per table.

Both drivers read only. Table writes return an `Unsupported` error, and so do `sum`, `min` and
`max`. Send a write yourself with `RestApi::http_request` or `GraphqlApi::post_graphql`. These
go through the same client, so they share its cap, breaker and observer.

## Pages

- [REST: the `api:` block](./http-apis/rest-yaml.md): endpoint templates, references, column
  types, and the builder settings that sit beside them.
- [GraphQL: the `graphql:` blocks](./http-apis/graphql-yaml.md): every table and column key,
  dialects, paging, counting, search and references.
