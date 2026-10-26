# Changelog

## 2.2.2 — 2026-10-26

- Tests of one behaviour are folded into one test each (bearer sources, GraphQL requests, errors,
  paging and escaping, REST conditions, retries and the breaker, vista refusals) and tests another
  test already covers are dropped: 343 tests become 301 with the same coverage. No API or behaviour
  change.

## 2.2.1 — 2026-10-25

- A test whose assertions another test already makes is dropped. No API or behaviour change.

## 2.2.0 — 2026-10-18

- Requires vantage-table 3.0.0.
- A bearer token can come from a `BearerSource` (`Arc<dyn Fn() -> Option<String> + Send + Sync>`),
  asked each time a request goes out, retries included. `None` sends no `Authorization` header.
  Until now a datasource had one fixed header or one refresher per client, so a client shared by
  several sessions sent the same token for all of them.
- `RestApiBuilder::bearer_source` and `GraphqlApiBuilder::bearer_source` set one; like `auth` and
  `auth_refresher`, each replaces the others.
- `RestApi::with_bearer_source` and `GraphqlApi::with_bearer_source` return a clone that sends its
  own source's token and shares the original's client: connection pool, parallel cap, breaker
  and observer. One clone per session costs no extra connections. The client still adds an
  `auth_refresher`'s header itself, so build the shared API without one when clones bring tokens.
- A `401` on a request whose token came from a source is a final error, not refreshed; the next
  request asks the source again.
- `in_values_condition`: GraphQL builds an `in` filter. REST takes one value, sent as the same
  `field=value` param as an equality, and refuses any other list as `Unsupported`, since a URL
  filter param carries one value and dropping the condition would widen the set.
- `describe_condition` and `condition_column` are implemented for REST and GraphQL.
- REST `list`, `count` and `fetch_window` run the table's `BeforeSelect` hooks before any request
  goes out.

## 2.0.1 — 2026-10-15

- The in-crate unit tests move to the blackbox suite under `tests/`, duplicate tests are dropped,
  and public behaviour the docs promise but nothing tested gets a test. No API or behaviour change.
- The README names `driver_name()`, and the book now says a Generic-dialect `contains` filter is
  refused as unsupported.

## 2.0.0 — 2026-10-14

### Breaking changes

- `ModelResolver` is removed; `RestApiVistaFactory::with_model_resolver` takes
  `Arc<dyn VistaResolver>`. A closure or a `Catalog` fits.
- `GraphqlSpecResolver` is removed; `GraphqlApiVistaFactory::with_resolver` takes
  `Arc<dyn SpecResolver<GraphqlApiVistaSpec>>`. A closure passed there needs an annotated
  parameter (`|name: &str| ..`).
- Both shells implement `TableShell` 2.0: `fetch_page` and GraphQL's page-number `fetch_next` are
  gone, neither driver advertises `Cursor`, and `fetch_window` returns a `Page`. A REST
  `total_key` becomes `Page.total` from the same response.
- Neither driver advertises writes, so Vista refuses them before the shell.
- `set_order`, `set_search` and `fetch_window` refuse with `unsupported(..)` when their capability
  is off.
- REST conditions accept declared columns and endpoint `{placeholder}` names only; any other
  column is an error. A non-`Eq` operator, or an `Eq` with a non-scalar value, is refused so it is
  evaluated locally.
- GraphQL `Like` on the generic dialect is `unsupported(FilterOps)`; it was `Unimplemented`.
- A dotted path into a declared column is refused as `Unimplemented` on REST and on GraphQL with
  filter push-down. An unknown root is "Unknown column for condition" with a `field` key.
- GraphQL types each text operand by its column for every caller: an integer column takes an
  integer, and a non-integer text there is an error before any request.
- A has-one target without an id column is an error at traversal; REST fell back to `"id"`.
- A REST client-side filter keeps a row lacking the field only for a `request_param` column, an
  endpoint placeholder or an undeclared field. A `hidden` column is a row field.
- `preview()` is `{query, note}`. REST: `query` is `GET <url>`, plus `Authorization: <set>` when
  an auth header is configured, and `note` lists client filters and unresolved conditions.
  GraphQL: `query` is `POST <endpoint>` and the document, with no `note`.
- A name the REST registry or GraphQL spec resolver doesn't know is `is_not_found()`; the REST
  traversal message is "No registered spec for model name".
- GraphQL quicksearch reads the columns flagged `Searchable`, or, when none is, every text column
  other than the id.

### Other changes

- A REST API that doesn't page and a GraphQL root without paging answer `fetch_window(0, n)` from
  one unpaged listing, with the listing's length as `total`.
- REST `list()` applies `set_order`; it ignored it.
- REST and GraphQL lower YAML references through `ReferenceSpec::to_reference`, so `keys:` gives
  a multi-key join.
- A reference's `target` is its spec name for YAML and its table name for a typed relation, so
  `catalog.vista(&reference.target)` resolves.
- A REST spec's `label` flag reaches the vista column.
- A failed removal of the previous GraphQL quicksearch condition propagates from `set_search`.

Migration guide: https://vantage-ui.com/book/migration-2.0.html

## 1.0.0 — 2026-10-11

- **Breaking:** `#[non_exhaustive]` on `FilterDialect`, `GraphqlOp`, `GraphqlCondition`,
  `ResponseShape`, `FilterStrategy`, `CountMode`, `PaginationMode`, `Supports`, `PaginationParams`,
  `OrderingParams`, `RenderedQuery` and the YAML block structs; build them with `Default`,
  `new` and `with_*`.
- **Breaking:** internal modules are private (`graphql::{api, condition, operation, select,
  impls}`, `rest::{api, operation, table_source}`, the vista `source` modules); types are
  re-exported from the crate root, `graphql`, `rest` and `prelude`.
- **Breaking:** `GraphqlSelect` fields are private (read accessors added); `group_by` and
  `distinct` removed.
- **Breaking:** YAML `dialect:` is typed: `generic` or `hasura`, lowercase only (was lower-cased);
  an unknown value fails to parse.
- **Breaking:** `PaginationParams` has `mode: PaginationMode` (`Page` | `Skip`) instead of
  `skip_based: bool`.
- **Breaking:** `NoApiExtras`, `NoGraphqlExtras`, `RestApi::with_auth` and
  `GraphqlApiBuilder::client` removed; use `builder().auth(..)` and `http_client(..)`.
- **Breaking:** requests time out after 30 s by default (no timeout before); `request_timeout(..)`
  and `no_request_timeout()` on both builders.
- **Breaking:** the default HTTP client is built on the first request, not at construction.
- **Breaking:** re-exported pool types (`TransportEvent` and its variants, `ClientError`,
  `ErrorKind`, `BreakerState`, `CallPolicy`, `RetryMode`) are `#[non_exhaustive]`; build them with
  their constructors. `ErrorKind::Closed` removed.
- **Breaking:** `#[non_exhaustive]` on `FieldCondition` and `GraphqlCondition::DeferredField`; build
  them with `FieldCondition::new` and `GraphqlCondition::deferred_field`.
- **Breaking:** `OrderingParams::value` and `RestApi::serves_windows` are no longer public.
- **Breaking:** GraphQL paging is advertised only with filter push-down (`paginate` and `filter`).
- **Breaking:** writes and `sum` / `min` / `max` return `Unsupported` errors on both drivers.
- **Breaking:** a row without an id (GraphQL: a null or empty id) is keyed `~row:<pos>`; two
  rows with the same id in one response are an error.
- **Breaking:** GraphQL names (fields, root field, arguments, filter keys, order and count fields)
  are validated; an invalid name fails before any request.
- GraphQL vistas with `paginate: true` serve `fetch_page`, `fetch_window` and `fetch_next`;
  Hasura paged reads order by the id column when no order is set.
- Hasura `count: aggregate` opts in to counting through `<root>_aggregate` (counts still list by
  default); `count_field:` overrides its name. A rejected aggregate is an error naming
  `count: list`.
- `GraphqlApi::can_page`.
- A spec's `graphql:` block overrides the builder's `Supports` key by key.
- GraphQL ordering reaches the server (`Vista::add_order`); dotted columns are refused.
- GraphQL quicksearch covers `searchable:` columns, or else string-typed non-id columns; none
  matches nothing. `%`, `_` and `\` are escaped.
- GraphQL get-by-id honours `Supports` and sends an int id column's value as an integer.
- Hasura `contains` renders `_ilike`.
- `GraphqlApiTableShell` implements `clone_shell`.
- YAML `references:` on GraphQL specs, with `register_yaml`, `build` and `with_resolver`
  (`GraphqlSpecResolver`).
- YAML column `field:` aliases a column to a different server field.
- GraphQL strings escape every control character.
- `Supports`, `CountMode`, `RetryPolicy` and `ResilientClientBuilder` are exported from the crate
  root.
- REST `column_table_values_expr` lists the table and returns the column (was a panic).
- `RestApi::http_request` returns an error for a bad header name or value (was a panic).
- Depends on `vantage-api-pool` 1.0.0.

## 0.6.20 — 2026-10-04

- The REST and GraphQL vista factories use the shared spec helpers.
- REST conditions resolve deferred values through `Expression::resolve_deferred`.
- Depends on `vantage-expressions` 1.0.1, `vantage-table` 1.1.1, `vantage-vista` 1.1.1.

## 0.6.19 — 2026-10-03

- Spec `lazy:` columns become Vista computed columns, on GraphQL too.
- Depends on `vantage-table` 1.1.0, `vantage-vista` 1.1.0.

## 0.6.18 — 2026-10-03

- Depends on `vantage-core` 1.0.0, `vantage-types` 1.0.0, `vantage-expressions` 1.0.0,
  `vantage-dataset` 1.0.0, `vantage-table` 1.0.0, `vantage-vista` 1.0.0, `vantage-api-pool` 0.6.3.

## 0.6.17 — 2026-09-27

- `auth_refresher(AuthRefresher)` on the REST and GraphQL builders: the
  bearer token is fetched lazily and fetched again after a 401.
- A REST vista built with `pagination_params(...)` serves windowed fetches
  without a `total_key`; a short page marks the end of the data.
- Fix: a window starting mid-page on a page-numbered API returned the page's
  first rows, and a page that lost rows to client-side filters or repeated
  ids came back short, ending the data early.
- `auth(...)` set after `auth_refresher(...)` replaces the refresher.
- A bare JSON object response is read as one row.

## 0.6.16 — 2026-09-16

- `RestApi::builder().ordering(OrderingParams)`: the API's sort query param
  (`?ordering=col`, prefix for descending). With it the REST vista reports
  `can_order`, every column is orderable, and a pushed sort is sent with each
  windowed fetch instead of being applied client-side.
- A REST call that gives up on a transient failure (5xx, network, open
  breaker) logs at debug; only a final answer (4xx, auth) warns.

## 0.6.15 — 2026-09-16

- REST and GraphQL requests go through `vantage-api-pool`'s `ResilientClient`
  (parallel cap, growing-cooldown breaker, rate limit, observer hook).
- The retry policy follows `vantage_core::Priority`: `Essential` retries until
  the awaiting future is dropped, `Background` (the default) makes one attempt.
- Errors from a non-2xx response carry `status`, `attempts`, `kind` and the
  first 500 chars of the server's `body`.
- `RowsPulled` is reported after each parsed page.
- Failures are now reported as `API request failed` / `GraphQL request failed`
  with a `kind` attribute (`status`, `transport`, `breaker_open`, `auth`),
  replacing the old `… returned error status` messages.
- Retries sleep on the tokio timer, so the runtime must have time enabled.

## 0.6.14 — 2026-09-08

- `add_op_condition` reports `Unimplemented` for `FilterOp::Like` — no
  GraphQL dialect here spells a pattern match, so the consumer evaluates it
  locally rather than having the filter silently dropped.

## 0.6.13 — 2026-09-05

- Follows vista 0.6.25: a lazy column's script compiles once through
  `lazy_value_closure`, and a script that does not parse is reported when the
  column is declared rather than on first read.

## 0.6.12 — 2026-08-16

- `GraphqlApiTableShell` implements `preview_query`, returning the endpoint and
  the query document.
- `RestApiTableShell` implements `preview_query`, returning the `GET` a read
  would issue. It shares the URL builder with the real fetch path, so a previewed
  URL cannot drift from the one that gets sent. Conditions that never reach the
  server under `FilterStrategy::Client` are reported separately as
  `client_side_filters` — the difference between a filter you configured and a
  filter the API applies.
- `RestApiTableShell::preview_query` no longer collapses to an error when a URI
  template placeholder is filled by a condition that resolves at fetch time. The
  real fetch awaits those before building the path; the preview cannot, so it
  shows the unfilled template and counts them under `unresolved_conditions`.

## 0.6.11 — 2026-08-16

- GraphQL columns may be dotted paths. `run.commit.hash` groups into a nested
  selection set at render time and flattens back to a dotted key on decode, so
  a schema that nests its scalars needs no relation machinery to read them. A
  null parent leaves the column present and null rather than missing.
- `args:` renders literal root-field arguments — for a field whose input object
  is non-null there was previously no way to query it at all. `response_path:`
  locates the rows inside a Relay-style connection, and shapes the query as
  well as the response, so the two can't drift apart.
- Per-table `filter`/`order`/`search`/`paginate` declare what a root field
  really accepts, defaulting to what the dialect implies. The shell implements
  `add_op_condition`/`add_order`/`add_search`/`clear_*` and the factory
  advertises the matching capabilities; previously every capability was left
  false, so consumers sorted and searched in memory even against a Hasura
  schema that could have done it server-side.
- With `filter: false` — a root field that takes no arguments at all —
  conditions are applied client-side over the decoded rows instead of being
  dropped. Vista treats equality push-down as universal, so dropping them
  silently widened results; rendering them produced a query the server
  rejected. A filter shape that can't be evaluated row-wise now errors rather
  than quietly matching everything. A deferred filter value resolves once for
  the whole set rather than per row, since resolving it fetches the parent
  table; LIKE anchors are honoured, so `foo%` is a prefix rather than a
  substring.

## 0.6.10 — 2026-08-12

- **Bug fix:** the REST and GraphQL `TableShell`s resolve relations whose target
  constructor returns a typed table. `get_ref_target` went through an
  entity-typed downcast that failed, so a reference field's list of eligible
  rows came back as an error.

## 0.6.9 — 2026-07-28

- Every REST round trip is timed. Over a second it logs `slow REST GET` at info
  with the URL and elapsed time; otherwise `REST GET done` at debug. Both carry
  `count_probe`, which distinguishes a request for rows from one made only to
  learn a total — the distinction that identified an 8.5s startup as a separate
  count query on the open path.

## 0.6.8 — 2026-07-23

- REST and GraphQL `TableShell`s implement `get_ref_target` — the bare,
  unconditioned relation target the edit-form reference dropdown lists. REST
  handles both YAML-declared references (the resolver-built child with no join
  condition) and hand-coded `with_one`/`with_many` registrations; GraphQL
  forwards through the factory like `get_ref`. Previously this path returned
  `Unimplemented`.

## 0.6.7 — 2026-07-20

- `lazy:` computed columns on REST tables, behind the new `rhai` feature — a
  Rhai script runs per returned record via `vantage-vista`'s shared
  lazy-expression evaluator. The windowed fetch path (`fetch_window`) applies
  them directly, since it bypasses `list_values` where they'd normally run.

## 0.6.6 — 2026-07-16

- GraphQL CBOR bridges and the REST HTTP→CBOR boundary use the shared `vantage-types`
  walker (total — no error branch, and tagged values no longer collapse to `null`).

## 0.6.5 — 2026-06-29

- REST: the count probe is quieter under `.debug(true)`. The count query and its
  `window (0, 1)` probe GET now log at `debug`, while real data fetches stay at
  `info` — so the log shows the fetches that matter without the count-probe noise.

## 0.6.4 — 2026-06-26

- REST: with `.debug(true)`, each GET now logs the resolved request URL
  (`target: "vantage_api_client::rest"`) so a failing query can be replayed by hand.

## 0.6.3 — 2026-06-25

- `AnyGraphqlType` declares `null_when: serde_json::Value::Null`, so it implements `InvariantValue`
  (vantage-types 0.6.2) and participates in vantage-table's set-invariant enforcement. (REST uses
  `ciborium::Value` directly, which already implements the trait.)

## 0.6.2 — 2026-06-20

- REST: fix URL construction when a table path already carries a query string
  (e.g. `launches/?mode=detailed`). Pagination and condition params now join with
  `&` instead of emitting a second `?`, which the server rejected (HTTP 500).

## 0.6.1 — 2026-06-18

- REST lazy-load: `RestApiBuilder::total_key`/`debug`, `RestApi::fetch_window_records`/`fetch_total`,
  a `RestApiTableShell::fetch_window` impl + envelope-total count, and `can_fetch_window` advertised
  when a `total_key` is configured. Windows map onto skip-based (raw offset) or page-based
  (`offset/limit+1`) pagination.

## 0.6.0 — 2026-06-17

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
