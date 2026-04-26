# Changelog

## 0.1.2 — 2026-04-26

- New `RestApi::builder(base_url)` builder for picking response shape and pagination conventions. The legacy `RestApi::new(url)` still works and matches the 0.1.x default (`{ "data": [...] }` wrapper).
- New `ResponseShape` enum: `BareArray` (most public APIs), `Wrapped { array_key }` (legacy), `WrappedByTableName` (DummyJSON-style).
- New `PaginationParams` with `page_limit` (1-based page index, the JSON Server / JSONPlaceholder convention `_page` / `_limit`) and `skip_limit` (0-based item offset, DummyJSON convention `skip` / `limit`) constructors. `RestApi` now appends pagination to the URL automatically when a `Pagination` is set on the wrapping `Table`.
- New `eq_condition(field, value)` helper for building eq-conditions on `Table<RestApi, _>`. Required because Rust's orphan rule blocks `Expressive<serde_json::Value>` impls for primitive types in this crate. Conditions added via `add_condition` are pushed into the URL as query params (`?field=value`) on `list_values` — multiple conditions AND together. Non-eq conditions are silently skipped for now; gt/lt/like/etc. would need their own translators.
- Adds `urlencoding` as a runtime dep so query params get encoded properly.

## 0.1.1 — 2026-04-19

- Pinned dependency versions for crates.io publishing.
