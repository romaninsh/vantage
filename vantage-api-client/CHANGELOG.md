# Changelog

## 0.6.2 — unreleased

- REST: fix URL construction when a table path already carries a query string
  (e.g. `launches/?mode=detailed`). Pagination and condition params now join with
  `&` instead of emitting a second `?`, which the server rejected (HTTP 500).

## 0.6.1 — unreleased

- REST lazy-load: `RestApiBuilder::total_key`/`debug`, `RestApi::fetch_window_records`/`fetch_total`,
  a `RestApiTableShell::fetch_window` impl + envelope-total count, and `can_fetch_window` advertised
  when a `total_key` is configured. Windows map onto skip-based (raw offset) or page-based
  (`offset/limit+1`) pagination.

## 0.6.0 — unreleased

- Coordinated 0.6 release; internal dependencies realigned to 0.6. No public API changes.

## 0.5.7 — 2026-06-06

### Changed

- Formatting only (`cargo fmt`): no functional or API change.

## 0.5.6 — 2026-06-02

- Track `vantage-table`'s new `TableSource::Source` associated type (set to `String`; no
  user-visible change).

## 0.5.5 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.5.4 — 2026-05-23

- Doc comment touch-ups tracking
  [vantage-table 0.5.2](https://docs.rs/vantage-table/0.5.2/vantage_table/)'s `AnyTable` removal. No
  code change.

## 0.5.3 — 2026-05-23

- Dep-pin bump on `vantage-dataset` to 0.5; tracks the `ImTable` / `ImDataSource` parametrization.
  No public API change.

## 0.5.2 — 2026-05-23

- GraphQL Vista shell rewritten on a typed `Table<GraphqlApi, EmptyEntity>` — `AnyTable` is gone
  from the GraphQL path. `GraphqlApiTableShell` converts `AnyGraphqlType` ↔
  [`CborValue`](https://docs.rs/ciborium/latest/ciborium/value/enum.Value.html) at the Vista
  boundary via the symmetric `From` impls; the typed table keeps the native value flow for filters
  and reference traversal.
- `GraphqlApiTableShell::get_ref` now resolves children through
  [`Table::get_ref_from_row`](https://docs.rs/vantage-table/0.5.0/vantage_table/table/struct.Table.html#method.get_ref_from_row) +
  a fresh `GraphqlApiVistaFactory::from_table` wrap — same row-based traversal pattern as REST /
  MongoDB / SQL.
- [`GraphqlApi::eq_value_condition`](https://docs.rs/vantage-table/0.5.0/vantage_table/traits/table_source/trait.TableSource.html#method.eq_value_condition)
  implemented so `Reference::resolve_from_row` can push a row-derived `AnyGraphqlType` join key
  without a string round-trip.
- `GraphqlApiTableShell` implements `get_ref_kinds` via the typed table's reference registry. Vista
  metadata's `references` map now carries real cardinality flags (`HasOne` / `HasMany`) instead of
  the placeholder `HasMany`; target name and FK column remain blank because the typed `Reference`
  doesn't expose them.
- Removed `graphql/vista/any_shell.rs` — no remaining consumers.
- Removed `tests/any_table.rs` — both halves are gone now that neither shell routes through
  `AnyTable`.

## 0.5.1 — 2026-05-23

- REST Vista shell no longer routes typed `Table::with_many` / `with_one` references through
  `AnyTable`. `RestApiTableShell` now carries a typed `Table<RestApi, EmptyEntity>` and
  `TableShell::get_ref` resolves children via
  [`Table::get_ref_from_row`](https://docs.rs/vantage-table/0.5.0/vantage_table/table/struct.Table.html#method.get_ref_from_row) +
  a fresh `RestApiVistaFactory::from_table` wrap — mirroring the pattern used by the MongoDB, SQL,
  and SurrealDB drivers.
- [`RestApi::eq_value_condition`](https://docs.rs/vantage-table/0.5.0/vantage_table/traits/table_source/trait.TableSource.html#method.eq_value_condition)
  implemented so `Reference::resolve_from_row` can push a row-derived `CborValue` join key onto a
  child REST table without a string round-trip.
- `AnyTableShell` is no longer re-exported from `vantage_api_client::rest` (or the crate root /
  `prelude`). The legacy adapter survives as a `pub(crate)` helper under `graphql::vista` only as
  long as the GraphQL shell still wraps `AnyTable`; the matching GraphQL rewrite ships in the next
  release.
- `tests/any_table.rs` drops the REST half — REST no longer participates in the
  `AnyTable::from_table` blanket bridge from the Vista layer. The GraphQL half stays until the
  GraphQL shell is rewritten.

## 0.5.0 — 2026-05-23

- Bumped to the 0.5 line to track
  [vantage-table 0.5.0](https://docs.rs/vantage-table/0.5.0/vantage_table/)'s opening of the
  `AnyTable` decommission cycle, aligning with the rest of the workspace. No code changes beyond the
  dependency pin.

## 0.1.9 — 2026-05-18

- Tracks [vantage-vista 0.4.10](https://docs.rs/vantage-vista/0.4.10/vantage_vista/)'s
  schema-on-source refactor. `RestApiTableShell`, `GraphqlApiTableShell`, and `AnyTableShell` now
  own their
  [`VistaMetadata`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/struct.VistaMetadata.html)
  and implement the new `columns` / `references` / `id_column` shell methods. Factory call sites —
  `RestApi::vista_factory(...).from_yaml(...)`, `AnyTableShell::into_vista_with(...)` — are
  unchanged.
- Pins `vantage-vista = "0.4.10"`.

## 0.1.8 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.1.7 — 2026-05-16

- REST and GraphQL `TableShell::get_ref` impls updated for
  [`Vista::get_ref(relation, &row)`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/struct.Vista.html#method.get_ref)'s
  row-based signature.
- REST's YAML-driven traversal drops the `DeferredFn`-based parent-fetch dance: with the parent row
  passed in, the shell reads the join field directly and pushes a plain eq-condition on the
  resolver-built child Vista. The `with_model_resolver` callback survives for cross-driver inventory
  routing.
- The YAML factory's `ReferenceKind::HasForeign → HasMany` workaround is gone now that
  `ReferenceKind` is two-variant; declared cardinality flows straight through to the child Vista.
- Pins `vantage-vista = "0.4.7"`, `vantage-table = "0.4.10"`.

## 0.1.6 — 2026-05-16

GraphQL joins REST inside `vantage-api-client`. The crate is now home to two protocol adapters, with
a YAML-driven Vista bridge for each. The example wires the SpaceX public GraphQL API end-to-end.

- **Breaking**: REST internals moved under `vantage_api_client::rest::*`. Crate-root re-exports
  (`RestApi`, `RestApiBuilder`, `ResponseShape`, `PaginationParams`, `eq_condition`,
  `AnyTableShell`, `RestApiTableShell`, `RestApiVistaFactory`, `RestApiVistaSpec`, `NoApiExtras`)
  are unchanged, so most consumers compile without edits. Code that imported through
  `vantage_api_client::vista::*` directly now lives at `vantage_api_client::rest::vista::*`.
- **New**:
  [`GraphqlApi`](https://docs.rs/vantage-api-client/0.1.6/vantage_api_client/struct.GraphqlApi.html)
  — POST-based GraphQL data source. Renders typed query documents with inline filters and
  `$limit`/`$offset` variables. Two filter dialects ship out of the box: `Hasura`
  (`where: { field: { _eq: v } }`) and `Generic` (`(find: { field: v })`, for hand-rolled schemas
  like SpaceX).
- **New**:
  [`GraphqlApiVistaFactory`](https://docs.rs/vantage-api-client/0.1.6/vantage_api_client/struct.GraphqlApiVistaFactory.html)
  — builds a Vista from either a typed `Table<GraphqlApi, E>` or a hand-maintained YAML schema
  (`build_from_spec`). The YAML carries per-table `graphql:` blocks for `root_field`, `dialect`, and
  `filter_arg` overrides; column types map to typed Vantage columns (`int`, `bigint`, `bool`,
  `float`, `string`, `datetime`, `date`, `time`, `uuid`, `json`).
- **New**: typed condition operators via
  [`GraphqlOperation`](https://docs.rs/vantage-api-client/0.1.6/vantage_api_client/trait.GraphqlOperation.html)
  — `.eq()`, `.ne()`, `.gt()`/`.gte()`/`.lt()`/`.lte()`, `.in_()`, `.like()`/`.ilike()`,
  `.is_null()`/`.is_not_null()`. Blanket-implemented over `Expressive<T>` so typed columns get them
  for free.
- **New**: relationship traversal via `with_many`/`with_one`. The adapter peels existing parent
  eq-conditions in-sync and falls back to async `DeferredField` resolution otherwise; both paths
  render dialect-correct (`_eq` for Hasura, flat for Generic).
- **New**:
  [`AnyGraphqlType`](https://docs.rs/vantage-api-client/0.1.6/vantage_api_client/struct.AnyGraphqlType.html)
  — type-erased value wrapping `serde_json::Value` with a 15-variant scalar enum (`Null`, `Bool`,
  `Int`, `BigInt`, `Float`, `String`, `Id`, `DateTime`, `Date`, `Time`, `Uuid`, `Decimal`, `Json`,
  `Object`, `Array`). Built-in Rust impls cover `bool`, `i32`/`i64`/`f64`, `String`/`&str`,
  `chrono::DateTime<Utc>`/`NaiveDate`/`NaiveTime`, `uuid::Uuid`, `serde_json::Value`,
  `Vec<AnyGraphqlType>`, `IndexMap<String, AnyGraphqlType>`. Downstream crates can plug their own
  types onto existing variants, or call `vantage_type_system!` a second time for a fresh variant set
  (e.g. a `vantage-graphql-geo` extension).
- **New**: `prelude` module for one-shot imports of both protocols. CBOR bridges round-trip cleanly
  through `AnyTable`, so a `Vec<AnyTable>` can mix REST and GraphQL tables.
- **New example**: `examples/graphql_spacex.rs` — YAML-driven CLI over the public SpaceX GraphQL
  API. Ten entities (`launches`, `rockets`, `capsules`, `cores`, `ships`, `payloads`, `missions`,
  `dragons`, `landpads`, `launchpads`) in `examples/schema/*.yaml`. Run
  `cargo run --example graphql_spacex -- launches`. Endpoint overridable via `$SPACEX_ENDPOINT`.
- Dependencies added: `chrono`, `uuid`, `paste` (runtime).

## 0.1.5 — 2026-05-15

YAML-driven model definitions: describe a REST resource in YAML, accumulate a registry, and traverse
references — including across models defined in separate YAML files — without writing Rust factory
closures.

- [`RestApiVistaFactory::register_yaml(&str)`](https://docs.rs/vantage-api-client/0.1.5/vantage_api_client/struct.RestApiVistaFactory.html#method.register_yaml)
  parses a `RestApiVistaSpec` and stashes it under `spec.name`.
  [`build(name)`](https://docs.rs/vantage-api-client/0.1.5/vantage_api_client/struct.RestApiVistaFactory.html#method.build)
  materialises a `Vista` on demand; references resolve through the same registry so a parent's
  `:relation` traversal finds the child's spec without further wiring.
- New table-level `api` block carries `endpoint: parent/{parentId}/child` for APIs that require
  path-based filtering. Defaults to `spec.name` when absent — declaring `name: users` is enough for
  a plain `/users` resource.
- [`RestApiVistaFactory::with_model_resolver(Arc<dyn Fn(&str) -> Result<Vista>>)`](https://docs.rs/vantage-api-client/0.1.5/vantage_api_client/struct.RestApiVistaFactory.html#method.with_model_resolver)
  installs a callback that overrides the internal registry — for cross-driver setups (e.g. a UI
  shell whose inventory layer routes some models through SQL and others through REST) the resolver
  decides which factory builds each name.
- Cross-cutting public types:
  [`ApiTableExtras`](https://docs.rs/vantage-api-client/0.1.5/vantage_api_client/struct.ApiTableExtras.html),
  [`ApiTableBlock`](https://docs.rs/vantage-api-client/0.1.5/vantage_api_client/struct.ApiTableBlock.html),
  `ApiColumnExtras`, `ApiReferenceExtras`, and
  [`ModelResolver`](https://docs.rs/vantage-api-client/0.1.5/vantage_api_client/type.ModelResolver.html).
  `NoApiExtras` stays as a legacy alias.
- New `examples/jsonplaceholder_yaml.rs` — same demo as `jsonplaceholder` with all three models in
  `data/jsonplaceholder/*.yaml`. Run
  `cargo run --example jsonplaceholder_yaml -- users id=1 :albums :photos`.

## 0.1.4 — 2026-05-14

REST API now speaks [`ciborium::Value`](https://docs.rs/ciborium/) end-to-end and bridges into the
universal [`Vista`](https://docs.rs/vantage-vista/0.4.5/vantage_vista/struct.Vista.html) surface.

- **Breaking**: `RestApi::Value` is now `ciborium::Value` instead of `serde_json::Value`.
  `eq_condition(field, value)` accepts anything that implements `Into<CborValue>` — so
  `eq_condition("userId", 1i64)` and `eq_condition("name", "Alice")` work without a wrapper macro.
  Existing callers passing `json!(...)` need to switch to the matching CBOR variant. Records
  returned from `list_values` carry `Record<CborValue>` now, which means master tables wrap into
  `AnyTable::from_table` directly — no JSON↔CBOR adapter boilerplate at the call site.
- URI templates in table names get substituted from eq-conditions at request time. Declare a child
  table as `Table::new("users/{userId}/albums", api)` and a parent's `id=1` condition lowers to
  `GET /users/1/albums`, with `userId` peeled out of the query string. Lets `with_many` / `with_one`
  traversal hit nested REST endpoints natively.
- [`related_in_condition`](https://docs.rs/vantage-api-client/0.1.4/vantage_api_client/struct.RestApi.html)
  is implemented: `with_many` re-keys an existing eq-condition onto the child's FK field, and
  `with_one` resolves the parent record on demand through a deferred condition that's executed at
  fetch time. No more `unimplemented!()` panic for REST API references.
- New
  [`vista_factory`](https://docs.rs/vantage-api-client/0.1.4/vantage_api_client/struct.RestApi.html#method.vista_factory)
  method returns a `RestApiVistaFactory` that wraps a typed `Table<RestApi, E>` as a `Vista`.
  Columns, id field, title fields, and references are harvested up front; the original entity stays
  attached for reference traversal.
- New `examples/jsonplaceholder.rs` demo — model-driven CLI over
  <https://jsonplaceholder.typicode.com> using the new Vista bridge and the `vista_cli` runner from
  [`vantage-cli-util 0.4.2`](https://docs.rs/vantage-cli-util/0.4.2/vantage_cli_util/). Run
  `cargo run --example jsonplaceholder -- users id=1 :albums`.

## 0.1.3 — 2026-05-09

Opt-in
[`RestApiBuilder::no_pagination()`](https://docs.rs/vantage-api-client/0.1.3/vantage_api_client/struct.RestApiBuilder.html#method.no_pagination)
for APIs that don't paginate. ([#230](https://github.com/romaninsh/vantage/pull/230))

```rust
let api = RestApi::builder(base_url).no_pagination().build();
```

FastAPI / Pydantic services often treat unknown query params as strict filters and silently return
empty rows, so adding `_page=1&_limit=50` makes the response empty. Setting this stops vantage from
sending those params, and a perpetual grid asking for page > 1 short-circuits to an empty result so
it actually marks itself exhausted instead of looping. Default behaviour is unchanged.

## 0.1.2 — 2026-04-26

`RestApi::builder` now configures response shape and pagination convention, so the same client works
against APIs with different conventions:

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

- `RestApi::builder(base_url)` builder. Methods: `.auth(value)`, `.response_shape(shape)`,
  `.pagination_params(params)`, `.build()`. The legacy `RestApi::new(url)` still works and matches
  the 0.1.x default.
- `ResponseShape::{BareArray, Wrapped { array_key }, WrappedByTableName}` — covers most public-API
  shapes (bare arrays like JSONPlaceholder/GitHub, wrapped-under-fixed-key like the legacy default,
  wrapped-under-table-name like DummyJSON).
- `PaginationParams::page_limit(page, limit)` — 1-based page index (JSON Server convention).
  `PaginationParams::skip_limit(skip, limit)` — 0-based item offset (DummyJSON convention).
  `RestApi` now appends pagination to the URL when a `Pagination` is set on the wrapping `Table`.
- `eq_condition(field, value)` helper for building conditions on `Table<RestApi, _>`. Required
  because Rust's orphan rule blocks `Expressive<serde_json::Value>` impls for primitive types in
  this crate. Conditions added via `add_condition` are translated to URL query params on
  `list_values` (`?field=value`); multiple conditions AND together. Non-eq operators are silently
  skipped for now.
- `urlencoding` joins the runtime deps so query params get encoded properly.

## 0.1.1 — 2026-04-19

- Pinned dependency versions for crates.io publishing.
