# Changelog

## 0.1.4 — 2026-05-11

Vista support behind the `vista` feature, so generic UIs that consume
[`Vista`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/struct.Vista.html)
can drive a REST endpoint the same way they drive SQLite or Mongo.

```rust
use vantage_api_client::RestApi;
use vantage_vista::VistaFactory;

let api = RestApi::builder("https://jsonplaceholder.typicode.com")
    .response_shape(vantage_api_client::ResponseShape::BareArray)
    .build();

// Wrap a typed table.
let typed = Table::<RestApi, EmptyEntity>::new("users", api.clone()).with_id_column("id");
let vista = api.vista_factory().from_table(typed)?;

// Or build it from YAML.
let yaml = include_str!("users.vista.yaml");
let vista = api.vista_factory().from_yaml(yaml)?;
```

- `RestApi::vista_factory()` returns a [`RestApiVistaFactory`](https://docs.rs/vantage-api-client/0.1.4/vantage_api_client/vista/struct.RestApiVistaFactory.html). The factory exposes `from_table<E>(Table<RestApi, E>) -> Result<Vista>` for the typed path and inherits `from_yaml(&str)` from [`VistaFactory`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/trait.VistaFactory.html) for the schema path.
- Read-only — `can_count: true`, `paginate_kind: PaginateKind::Offset`, every write capability `false`. Writes fall through to the default `TableShell` impl, which produces an `Unsupported` error consistent with the existing `TableSource` impl on `RestApi`.
- YAML extras: per-table `api: { endpoint }` (path segment override when it differs from the spec name) and per-column `api: { field }` (JSON field alias). Base URL, auth header, response shape, and pagination convention stay on the factory's `RestApi` since they're per-deployment.
- JSON ↔ CBOR translation at the row boundary via a `ciborium::Value::serialized` round-trip — same lossy edges (NaN, binary-as-string) you'd hit anywhere else and a no-op for vanilla REST traffic.
- `add_eq_condition(field, CborValue)` translates back to a `serde_json::Value` and pushes via the existing `eq_condition` helper, so the condition lands in the URL the same way hand-built ones do — `?field=value`, multiple conditions AND together.

## 0.1.3 — 2026-05-09

Opt-in [`RestApiBuilder::no_pagination()`](https://docs.rs/vantage-api-client/0.1.3/vantage_api_client/struct.RestApiBuilder.html#method.no_pagination) for APIs that don't paginate. ([#230](https://github.com/romaninsh/vantage/pull/230))

```rust
let api = RestApi::builder(base_url).no_pagination().build();
```

FastAPI / Pydantic services often treat unknown query params as strict filters and silently return empty rows, so adding `_page=1&_limit=50` makes the response empty. Setting this stops vantage from sending those params, and a perpetual grid asking for page > 1 short-circuits to an empty result so it actually marks itself exhausted instead of looping. Default behaviour is unchanged.

## 0.1.2 — 2026-04-26

`RestApi::builder` now configures response shape and pagination convention, so the same client works against APIs with different conventions:

```rust
use vantage_api_client::{RestApi, ResponseShape, PaginationParams, eq_condition};

// JSONPlaceholder: bare-array responses, JSON-Server pagination.
let api = RestApi::builder("https://jsonplaceholder.typicode.com")
    .response_shape(ResponseShape::BareArray)
    .pagination_params(PaginationParams::page_limit("_page", "_limit"))
    .build();

// DummyJSON: wrapped-by-table-name, skip-based pagination.
let api = RestApi::builder("https://dummyjson.com")
    .response_shape(ResponseShape::WrappedByTableName)
    .pagination_params(PaginationParams::skip_limit("skip", "limit"))
    .build();

// Legacy / authed: original 0.1.x default shape `{ "data": [...] }`.
let api = RestApi::builder("https://my-api.example.com")
    .response_shape(ResponseShape::Wrapped { array_key: "data".into() })
    .auth("Bearer …")
    .build();
```

Conditions get pushed into the URL automatically:

```rust
let mut comments = Table::<RestApi, EmptyEntity>::new("comments", api);
comments.add_condition(eq_condition("postId", json!(1)));
// list_values fetches: GET /comments?postId=1
```

What's new:

- `RestApi::builder(base_url)` builder. Methods: `.auth(value)`, `.response_shape(shape)`, `.pagination_params(params)`, `.build()`. The legacy `RestApi::new(url)` still works and matches the 0.1.x default.
- `ResponseShape::{BareArray, Wrapped { array_key }, WrappedByTableName}` — covers most public-API shapes (bare arrays like JSONPlaceholder/GitHub, wrapped-under-fixed-key like the legacy default, wrapped-under-table-name like DummyJSON).
- `PaginationParams::page_limit(page, limit)` — 1-based page index (JSON Server convention). `PaginationParams::skip_limit(skip, limit)` — 0-based item offset (DummyJSON convention). `RestApi` now appends pagination to the URL when a `Pagination` is set on the wrapping `Table`.
- `eq_condition(field, value)` helper for building conditions on `Table<RestApi, _>`. Required because Rust's orphan rule blocks `Expressive<serde_json::Value>` impls for primitive types in this crate. Conditions added via `add_condition` are translated to URL query params on `list_values` (`?field=value`); multiple conditions AND together. Non-eq operators are silently skipped for now.
- `urlencoding` joins the runtime deps so query params get encoded properly.

## 0.1.1 — 2026-04-19

- Pinned dependency versions for crates.io publishing.
