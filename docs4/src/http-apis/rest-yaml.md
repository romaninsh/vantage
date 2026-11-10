# REST: the `api:` block

A REST spec is a `RestApiVistaSpec`. Its only driver-specific key is the table-level `api:` block.
Columns and references use the universal spec keys, with no `api:` block of their own.

```yaml
name: albums
columns:
  id: { type: int, flags: [id] }
  userId: { type: int }
  title: { type: string, flags: [title] }
api:
  endpoint: users/{userId}/albums
references:
  photos: { table: photos, kind: has_many, foreign_key: albumId }
```

```rust,no_run
# use vantage_api_client::{ResponseShape, RestApi};
# const ALBUMS_YAML: &str = r#"
# name: albums
# columns:
#   id: { type: int, flags: [id] }
#   userId: { type: int }
#   title: { type: string, flags: [title] }
# api:
#   endpoint: users/{userId}/albums
# references:
#   photos: { table: photos, kind: has_many, foreign_key: albumId }
# "#;
# const PHOTOS_YAML: &str = r#"
# name: photos
# columns:
#   id: { type: int, flags: [id] }
#   albumId: { type: int }
#   title: { type: string, flags: [title] }
# "#;
# fn main() -> vantage_core::Result<()> {
let api = RestApi::builder("https://jsonplaceholder.typicode.com")
    .response_shape(ResponseShape::BareArray)
    .build();
let mut factory = api.vista_factory();
factory.register_yaml(ALBUMS_YAML)?;
factory.register_yaml(PHOTOS_YAML)?;
let albums = factory.build("albums")?;
# let _ = albums;
# Ok(())
# }
```

Building the factory sends nothing; the first read does. The block is compiled but not run,
because `RestApi::build` makes a `reqwest` client, which reads the system's proxy settings.

## Table keys

| Key | Type | Default | Meaning |
|---|---|---|---|
| `api.endpoint` | string | the spec `name` | Path under the API's base URL. May hold `{placeholder}` segments. |
| `api.read` | bool | `true` | `false` makes an insert-only table; see [Inserting](#inserting). |
| `api.insert` | block | none | How the table inserts; see [Inserting](#inserting). |

The whole `api:` block is optional. Unknown keys are a parse error.

### Endpoint templates

Each `{placeholder}` is filled at request time from the eq-condition on the column of that name,
and that condition is used up. Any eq-condition left over goes into the query string
(`?title=x`), or is applied to the fetched rows when the API was built with
`FilterStrategy::Client`. A placeholder with no condition to fill it is an error
(`No eq-condition provided for URI placeholder`), so nothing is sent.

So `users/{userId}/albums`, narrowed to `userId = 1`, fetches `GET /users/1/albums`. Traversal
from a user to its albums arrives at that URL without any code.

## Columns

| `type` | Column |
|---|---|
| `int`, `integer`, `i64`, `i32` | `i64` |
| `float`, `double`, `f64`, `f32` | `f64` |
| `bool`, `boolean` | `bool` |
| `string`, `text`, `str` (default) | `String` |
| `json`, `any` | raw CBOR value |

Any other type fails the build. `flags:` works as in every driver (`id`, `title`, `hidden`, …).
A column with a `lazy:` script is computed from each fetched record. It needs the crate's `rhai`
feature.

A row is keyed by its id column. A row whose id is missing, null, empty, or not a string or
number is keyed `~row:<position>`, where the position is absolute in the result. Two rows with
the same id in one response are an error.

## References

`references:` takes the universal form: `table`, `kind` (`has_one` / `has_many`) and
`foreign_key`, or `keys:` for a multi-column join. Traversal from a row reads each join value from
the parent row and gives the target one eq-condition per key. Each condition then fills a
placeholder in the target's `endpoint`, becomes a query parameter, or filters rows, by the rules
above.

- `has_one`: the parent's `foreign_key` value narrows the target's id column.
- `has_many`: the parent's id narrows the target's `foreign_key` column.
- `keys:`: a list of `{ from, to }` pairs, a parent column to a target column. When given, the pairs
  are the whole join and `foreign_key` is ignored. A deployment narrowed by both its product and
  its version is `keys: [{ from: product_id, to: product_id }, { from: version_id, to: version_id }]`.

The REST factory reads the `references:` map only. A column-level `references:` (the
`userId: { type: int, references: users }` form other drivers accept) is parsed but doesn't become
a relation, so declare REST joins in the map.

Targets are found by name. `register_yaml` keeps them in the factory's own registry. If the
models live across several drivers, `with_model_resolver(resolver)` takes an
`Arc<dyn VistaResolver>` that returns a built `Vista` for a name instead: a closure, or a
[`Catalog`](vantage_vista::Catalog).

## Inserting

A table with an `api.insert` block can be written to: each insert becomes a `POST`. The aim is
fire-and-forget targets such as analytics collectors, where you send events and never read them
back, so the block says nothing about updates or deletes. Those stay unsupported.

```yaml
name: events
columns:
  client_id: { type: string }
  name: { type: string }
api:
  read: false
  insert:
    path: mp/collect
    query: { measurement_id: G-ABC123 }
    accept: [204]
    batch: 25
    group_by: client_id
    body: '#{ client_id: record.client_id, events: records }'
    idempotency_key: Idempotency-Key
```

| Key | Default | Meaning |
|---|---|---|
| `api.read` | `true` | `false` makes an insert-only table: a list, get or count is refused before any request is made |
| `api.insert.method` | `POST` | Only `POST` is accepted; anything else fails the build |
| `api.insert.path` | the table's endpoint | Path under the base URL |
| `api.insert.query` | none | Query parameters sent with every insert |
| `api.insert.accept` | any 2xx | Statuses that count as success; any other is an error |
| `api.insert.batch` | `1` | Records per request |
| `api.insert.group_by` | none | Field whose value splits records into separate requests |
| `api.insert.body` | the record | A Rhai expression for the request body |
| `api.insert.idempotency_key` | none | Name of the header that carries the idempotency key |
| `api.insert.idempotent` | `false` | A repeated request is safe, with no key needed |
| `api.insert.encoding` | none | `gzip` compresses the body |

### Body templates

Without `body`, one record is sent as it is and a batch as an array. A template is for APIs that
want an envelope. It is a Rhai expression, compiled when the table is built, so a typo fails
there and not on the first event. It sees `records` (the batch), `record` (the first of them)
and `secret`, a map of values the host supplies with `RestInsertEndpoint::with_secrets`. Secrets
are bound as data. They are never pasted into the template text, so a key holding a quote or
code can't change what the template does. Templates need the crate's `rhai` feature.

### Batches and groups

`batch` caps the records in one request. With `group_by`, records are first split by that field
and then batched, so one request never mixes two values of it. PostHog and GA4 both want a batch
to belong to one client. A failed request stops the insert: later batches are not sent.

### Delivery

A retry is only safe when the receiver can recognise a repeat, so the client retries a `429`, a
`5xx` or a transport error only when you say so: either `idempotency_key` names a header that
carries the id of the request's first record, or `idempotent: true` says repeats are harmless.
Without either, a failed insert is reported once and not repeated.

A request is rendered once, and every attempt sends the same bytes, so a receiver that
de-duplicates on the body sees the same body. There are at most 5 attempts, with waits of 1, 2, 4
and 8 seconds between them. A `Retry-After` from the server replaces the wait, capped at 30
seconds. A status outside `accept` counts as a failure of the same kind, so a `503` is retried
and a `400` is not.

### Query auth

Some collectors take the key in the URL. GA4 reads `api_secret` from the query string. Set it on
the builder, since it belongs to the API and not to one table:

```rust,no_run
# use vantage_api_client::{ResponseShape, RestApi};
# const EVENTS_YAML: &str = r#"
# name: events
# columns:
#   client_id: { type: string }
#   name: { type: string }
# api:
#   read: false
#   insert:
#     path: mp/collect
#     query: { measurement_id: G-ABC123 }
#     accept: [204]
# "#;
# fn main() -> vantage_core::Result<()> {
let api = RestApi::builder("https://www.google-analytics.com")
    .response_shape(ResponseShape::BareArray)
    .query_auth("api_secret", "s3cr3t")
    .build();
let mut factory = api.vista_factory();
factory.register_yaml(EVENTS_YAML)?;
let events = factory.build("events")?;
# let _ = events;
# Ok(())
# }
```

`query_auth` adds the parameter to every request, reads included. The real value goes on the
wire, but a URL, header or error text that leaves the process shows `api_secret=***`: debug log
lines, errors, and the Vista preview. Headers named like a credential (`Authorization`, `x-api-key`)
show as `***` too. Every value under `api.insert.query` is treated as a possible key for the same
purpose, with its name left visible, and so is every value passed to `with_secrets` when an error
would otherwise echo it.

## Builder settings

The block is deliberately small. Everything that describes the API, rather than one table, is
set on `RestApi::builder(base_url)`:

| Builder method | Default | Meaning |
|---|---|---|
| `response_shape(ResponseShape)` | `Wrapped { array_key: "data" }` | Where the rows are: `BareArray`, `Wrapped { array_key }`, `WrappedByTableName` |
| `pagination_params(PaginationParams)` | `page_limit("_page", "_limit")` | Paging parameter names; `skip_limit("skip", "limit")` for 0-based offsets. Setting it enables windowed reads |
| `no_pagination()` | off | Send no paging parameters; the first page is the whole set |
| `total_key("count")` | none | Envelope key holding the total row count |
| `filter_strategy(FilterStrategy)` | `Query` | `Client` keeps non-path conditions off the wire |
| `ordering(OrderingParams::new("ordering", "-"))` | none | Sort parameter; makes every column orderable |
| `auth(..)` / `auth_refresher(..)` / `bearer_source(..)` | none | `Authorization` header: fixed, refreshed after a `401`, or asked for on every request |
| `query_auth("api_secret", value)` | none | A credential sent as a query parameter on every request, shown as `***` |
| `max_parallel`, `rate_limit`, `request_timeout`, `observer`, `http_client` | 4, none, 30 s, none, default | Transport; see the crate's `README_transport.md` |

When each signed-in session has its own token, build one `RestApi` and hand each session
`api.with_bearer_source(source)`: a clone that shares the client (connection pool, cap, breaker)
but asks its own source for the token on every request. Don't give the shared API an
`auth_refresher`, since the client adds that header itself and a clone can't drop it.

A REST Vista can always count and traverse. It advertises `Window` when the API pages (a
`total_key` puts the grand total on every page), and `Order` when `ordering` is set. It never
advertises `Search`, so the Vista refuses a quicksearch rather than send one the API can't apply.
A Dio over it searches the rows it holds instead, over the columns flagged `searchable`, or every
column when none is flagged.
