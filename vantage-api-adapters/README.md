# vantage-api-adapters

Axum routers that serve a [`vantage-diorama`](../vantage-diorama) `Dio` over
HTTP and mount [`vantage-action`](../vantage-action) actions as `POST`
routes. It is the server-side counterpart of `vantage-ui-adapters`: a UI
adapter binds a scenery to a widget, an API adapter binds it to an HTTP
endpoint, so any HTTP client becomes another scenery consumer.

```toml
[dependencies]
vantage-api-adapters = { version = "0.6.4", features = ["axum"] }
```

```rust,ignore
use vantage_api_adapters::axum_dio::DioRouter;

let api = DioRouter::new(dio.clone())
    .with_column("id", "id")         // JSON key <- record field
    .with_column("name", "name")
    .with_column("price", "price")
    .key_by("id")
    .with_page_size(50)
    .into_router();

let app = axum::Router::new().nest("/api/products", api);
```

## `DioRouter`

The router answers four requests, Kubernetes style:

- `GET /?offset=&limit=`: one page of the cached listing. It does not trigger
  hydration of augmented columns.
- `GET /?offset=&limit=&watch=true`: the same page as an NDJSON stream,
  `{"type":"ADDED","object":{..}}` per row and then `MODIFIED` as rows change.
  Each watch is a `TableScenery`; its columns are the demand and its page is
  the viewport, so naming an augmented column in `with_column` makes watches
  hydrate it. Closing the connection drops the scenery.
- `GET /{id}`: one record with every column, waiting for its augment columns.
- `GET /{id}?watch=true`: the record as a stream, via a `RecordScenery`.

Other options: `key_by(field)` diffs the listing watch by row id instead of
position (rows that leave produce `DELETED`), `with_id_map(..)` rewrites the
path id into the cache key, `with_record_projection(..)` shapes the detail
JSON, and `with_not_found_message(..)` sets the 404 text.

## `ActionRouter`

`ActionRouter::new(actions)` takes a `Vec<Arc<dyn ModelAction>>` and fails if
two actions share a table and name. `into_router()` mounts
`POST /<table>/actions/<name>` for table actions and
`POST /<table>/{id}/actions/<name>` for record actions. The request body is
the action input. `alias(table, name)` returns a handler for a path you choose:

```rust,ignore
let actions = ActionRouter::new(model_actions)?;
let app = Router::new()
    .route("/tag/{id}/register", actions.alias("tag", "register_tag")?)
    .merge(actions.into_router());
```

Errors map by kind: `NotFound` is 404, `IncorrectUsage` (including bad input)
is 400, `Conflict` is 409, everything else is a logged 500.

## Related crates

The book's `learn-7` to `learn-11` examples build on `DioRouter`.
