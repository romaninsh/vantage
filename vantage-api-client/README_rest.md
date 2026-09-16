# Connecting to a REST API

This guide takes a JSON-over-HTTP API from "I have a base URL" to typed
tables with traversal, pagination and an honest error when the API
misbehaves. It assumes nothing about the API's shape beyond returning JSON.
Where a choice depends on the API, the text says what to look for in a
`curl` response.

## 1. Look at one response first

```bash
curl -s 'https://api.example.com/users?limit=2' | head -c 400
```

Three things decide the builder settings:

| You see | Setting |
|---|---|
| `[ {…}, {…} ]` | `ResponseShape::BareArray` |
| `{ "data": [ … ] }` or `{ "results": [ … ], "count": 120 }` | `ResponseShape::Wrapped { array_key: "results" }` and, if there is a total, `.total_key("count")` |
| `{ "users": [ … ], "total": 100, "skip": 0, "limit": 30 }` | `ResponseShape::WrappedByTableName` and `.total_key("total")` |

Then paging: `?page=2&limit=30` (1-based page) is
`PaginationParams::page_limit("page", "limit")`; `?offset=60&limit=30` is
`PaginationParams::skip_limit("offset", "limit")`. The default is JSON
Server's `_page` / `_limit`.

## 2. Build the API

```rust
use vantage_api_client::{PaginationParams, ResponseShape, RestApi};

let api = RestApi::builder("https://api.example.com")
    .response_shape(ResponseShape::Wrapped { array_key: "results".into() })
    .pagination_params(PaginationParams::skip_limit("offset", "limit"))
    .total_key("count")
    .auth("Bearer …")
    .max_parallel(4)
    .build();
```

Build one `RestApi` per remote API and clone it into every table. The
parallel cap, rate limit, circuit breaker and health reporting live on the
`RestApi`, so one per table would give each table its own breaker.

Other knobs:

- `.filter_strategy(FilterStrategy::Client)` keeps eq-conditions off the
  wire and applies them to the parsed rows. For APIs that reject or ignore
  unknown query params; every page of the collection is fetched and
  filtered locally, so keep it to small joins.
- `.no_pagination()` for endpoints that return everything in one response
  and treat `_page` as a filter (FastAPI/Pydantic services).
- `.rate_limit(10.0)`, `.observer("users-api", obs)`, `.http_client(client)`:
  see [README_transport.md](README_transport.md).
- `.debug(true)` logs every request URL at `info` under
  `vantage_api_client::rest`.

## 3. Declare tables

```rust
use vantage_api_client::{RestApi, eq_condition};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

let mut users = Table::<RestApi, EmptyEntity>::new("users", api.clone())
    .with_id_column("id");
users.add_condition(eq_condition("status", "active"));
let rows = users.list_values().await?;        // GET /users?status=active&offset=0&limit=…
let one = users.get_some().await?;            // first row
```

The table name is the path under the base URL. It may carry a query string
(`"launches/?mode=detailed"`) and `{placeholder}` segments:

```rust
// GET /users/1/albums — the placeholder is filled from the eq-condition
let mut albums = Table::<RestApi, EmptyEntity>::new("users/{userId}/albums", api.clone());
albums.add_condition(eq_condition("userId", 1i64));
```

An eq-condition whose field matches a placeholder fills it; every other
eq-condition becomes `?field=value` (or a client filter). Only equality is
expressible on the wire: there is no server-side sort, search or range
filter in REST, so a Vista over a REST table sorts and searches what it
has loaded.

## 4. Traversal

```rust
let users = Table::<RestApi, User>::new("users", api.clone())
    .with_id_column("id")
    .with_many("albums", "userId", |api| Table::<RestApi, Album>::new("users/{userId}/albums", api));

let mut u = users.clone();
u.add_condition(eq_condition("id", 1i64));
let albums = u.get_ref_as::<Album>("albums")?;   // GET /users/1/albums
```

When the parent carries an eq-condition on the join field the child gets
the value directly. When it does not (a `with_one` from a row you already
hold), the child carries a deferred lookup that fetches the parent at
request time. The foreign key name is whatever the API wants as a query
parameter or placeholder: Launch Library's launches take `lsp__id`, so a
`has_many` from agencies is declared with `foreign_key: lsp__id`.

## 5. Lazy paging and counts

With `total_key` set the REST Vista advertises `can_fetch_window`: a paged
grid asks for one window at a time (`fetch_window(offset, limit)`) and sizes
its scrollbar from the envelope total. `get_count()` sends a one-row window
and reads the total. Without `total_key` every read is one request for the
server's default page and `get_count()` counts what came back.

## 6. From YAML

The same table as a Vista spec, for admin UIs and CLIs:

```yaml
name: albums
id_column: id
columns:
  id: { type: int, flags: [id] }
  title: { type: string, flags: [title] }
  userId: { type: int }
api:
  endpoint: users/{userId}/albums
```

```rust
use vantage_api_client::RestApiVistaFactory;

let mut factory = RestApiVistaFactory::new(api);
factory.register_yaml(USERS_YAML)?;
factory.register_yaml(ALBUMS_YAML)?;
let albums = factory.build("albums")?;
```

`endpoint` defaults to the spec's `name`. References declared in the spec
resolve through the registry, so `users id=1 :albums` traverses without
Rust.

## 7. Errors and retries

A failed request is a `VantageError` with the message `API request failed`
and the detail in its context: `url`, `attempts`, `kind` (`status`,
`transport`, `breaker_open`, `auth`), `status` when there was a response,
and up to 500 characters of the response `body`. `Display` prints them, so
`err.to_string()` contains the status code and the server's message.

Whether a failure is retried depends on who is waiting, not on the table:
wrap a call in `Priority::Essential.scope(..)` to retry until the future is
dropped; everything else makes one attempt. A `4xx` is never retried. The
full rules, including the circuit breaker and rate limits, are in
[README_transport.md](README_transport.md).

## 8. Seeing the request without sending it

`Vista::preview_query()` on a REST Vista (what the Vantage app's
`preview_query` MCP tool shows) renders the exact URL the fetch would send,
with the auth header masked, plus the client-side filters that would apply
afterwards. When a table returns nothing or ignores a condition, look here
first: a condition you expected on the wire may have been consumed by a
placeholder or moved to the client filter list.

## 9. Writes

`TableSource` writes (`insert`, `update`, `delete`) are refused with
`REST API is a read-only data source`. To call a write endpoint, use the
API's client directly:

```rust
use reqwest::Method;

let resp = api
    .http_request(Method::POST, "sim/launches", &[("Idempotency-Key", key)], Some(&body))
    .await?;
```

It applies the base URL and auth, shares the API's cap, breaker and health,
and reports `WritePushed` on success. Pair it with an idempotency key the
server honours if the call may be retried.

## Worked example

The `jsonplaceholder` and `jsonplaceholder_yaml` examples in this repo walk
users → albums → photos with typed factories and with YAML; `cities` shows
two-level traversal with an auth header. The Vantage app's `launch-control`
and `space` example apps are the same patterns driven from YAML against a
self-hosted and a public Launch Library server.
