# Changelog

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
