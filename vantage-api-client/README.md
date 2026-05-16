# vantage-api-client

REST and GraphQL backends for the [Vantage](https://github.com/romaninsh/vantage) persistence framework. Both adapters live in the same crate and share the same `TableSource` surface, so the application code that lists users from `/users` reads identically when those users are served from a GraphQL endpoint instead.

## What Problem Does Vantage Solve for HTTP APIs?

You've built an application against a SQL or document database and now a partner exposes their data through a third-party API. You could write a one-off HTTP client and a hand-rolled DTO layer, but that means duplicating the typed entities, condition composition, and relationship traversal you already have. You could try a generic GraphQL or REST client, but then your query logic doesn't compose with the rest of your Vantage code.

`vantage-api-client` gives you the same `Table<RestApi, E>` / `Table<GraphqlApi, E>` shape you already use for SQL or MongoDB — typed columns, conditions, pagination, references — talking to whatever HTTP endpoint happens to be on the other side. Mix REST and GraphQL tables in a single `AnyTable` collection. Drive them from YAML when you don't want a Rust factory per model.

## REST: Conditions Become Query Params

```rust
use vantage_api_client::{RestApi, ResponseShape, eq_condition};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

let api = RestApi::builder("https://jsonplaceholder.typicode.com")
    .response_shape(ResponseShape::BareArray)
    .build();

let mut users = Table::<RestApi, EmptyEntity>::new("users", api);
users.add_condition(eq_condition("id", 1i64));
// list_values fetches: GET /users?id=1
let rows = users.list_values().await?;
```

Configurable response shapes (`BareArray`, `Wrapped { array_key }`, `WrappedByTableName`) cover most public APIs without bespoke parsers. Eq-conditions land in the URL query string; pagination joins them as `_page` / `_limit` (JSON Server style) or `skip` / `limit` (DummyJSON style) depending on the builder choice. URI templates in table names (`Table::new("users/{userId}/albums", api)`) get substituted from parent eq-conditions at request time, so `with_many` traversal hits nested REST endpoints natively.

## GraphQL: Conditions Become Filter Objects

```rust
use vantage_api_client::{GraphqlApi, FilterDialect, GraphqlOperation};
use vantage_table::table::Table;
use vantage_table::column::core::Column;
use vantage_types::EmptyEntity;

let api = GraphqlApi::builder("https://spacex-api.fly.dev/graphql")
    .dialect(FilterDialect::Generic)
    .build();

let mut launches = Table::<GraphqlApi, EmptyEntity>::new("launches", api);
launches.add_column(Column::<String>::new("mission_name"));
launches.add_condition(Column::<String>::new("mission_name").eq("FalconSat"));
// POSTs: query { launches(find: {mission_name: "FalconSat"}) { id mission_name } }
let rows = launches.list_values().await?;
```

The query document gets rendered with inline filter values plus typed `$limit` / `$offset` variables. Two dialects ship out of the box:

- **`Hasura`** — `(where: { field: { _eq: v } })`, with `_and` / `_or` / `_not` and full operator coverage
- **`Generic`** — `(find: { field: v })`, equality only, for hand-rolled schemas like the SpaceX community API

The `GraphqlOperation` trait gives every typed column `.eq()`, `.ne()`, `.gt()`/`.gte()`/`.lt()`/`.lte()`, `.in_()`, `.like()`/`.ilike()`, and `.is_null()`. The operators produce a `GraphqlCondition` that renders dialect-correct at request time — the same `.eq("FalconSat")` lands as `{ mission_name: "FalconSat" }` for SpaceX and `{ mission_name: { _eq: "FalconSat" } }` for Hasura.

## Strong Types Over JSON

GraphQL wire format is JSON, but JSON loses the distinction between `Int` and `BigInt`, between a `String` and a `DateTime`. `AnyGraphqlType` wraps `serde_json::Value` with a 15-variant scalar enum (`Int`, `BigInt`, `Float`, `String`, `Id`, `DateTime`, `Date`, `Time`, `Uuid`, `Decimal`, `Json`, `Object`, `Array`, plus `Bool` and `Null`) so the framework keeps track of what a value *means*, not just what it serialises to:

```rust
use chrono::{DateTime, Utc};

let val = AnyGraphqlType::new("2026-05-16T12:00:00Z".parse::<DateTime<Utc>>()?);
assert_eq!(val.type_variant(), Some(GraphqlTypeVariants::DateTime));
assert_eq!(val.try_get::<DateTime<Utc>>().is_some(), true);
assert_eq!(val.try_get::<String>(), None); // Type-safe: won't coerce
```

Built-in Rust types covered: `bool`, `i32`/`i64`/`f64`, `String`/`&str`, `chrono::DateTime<Utc>`/`NaiveDate`/`NaiveTime`, `uuid::Uuid`, `serde_json::Value`, `Vec<AnyGraphqlType>`, `IndexMap<String, AnyGraphqlType>`.

Downstream crates can extend the type system in two ways. Plug a new Rust type onto an existing variant — `impl GraphqlType for Money { type Target = GraphqlTypeDecimalMarker; ... }` — and `.eq()`/`.gt()` on `Column<Money>` Just Works. For a richer variant set (e.g. a `vantage-graphql-geo` crate with `Point`/`Polygon` variants), call `vantage_type_system!` a second time with the same `value_type` and a wider variant list — the query builder and condition machinery are generic over the wrapper and come along for free.

## YAML-Driven Models

Both adapters carry a [`VistaFactory`](https://docs.rs/vantage-vista) that builds a `Vista` from a hand-curated YAML spec — useful for admin UIs, CLIs, and any setting where you'd rather describe an entity once than wire up a Rust factory closure per model.

```yaml
# launches.yaml
name: launches
id_column: id
columns:
  id:
    type: string
    flags: [id]
  mission_name:
    type: string
    flags: [title]
  launch_year:
    type: string
  launch_date_utc:
    type: datetime
  launch_success:
    type: bool
graphql:
  root_field: launches
  dialect: generic
  filter_arg: find
```

```rust
use vantage_api_client::{GraphqlApi, GraphqlApiVistaSpec};
use vantage_vista::VistaFactory;

let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(YAML)?;
let api = GraphqlApi::new("https://spacex-api.fly.dev/graphql");
let vista = api.vista_factory().build_from_spec(spec)?;

let launches = vista.list_values().await?;
```

The same shape exists for REST (`RestApiVistaSpec`) with an `api:` block carrying URL templates and a built-in registry. `RestApiVistaFactory::register_yaml(&str)` accumulates specs and resolves cross-model references through the registry — declare `users.yaml`, `albums.yaml`, `photos.yaml` and traversing `users id=1 :albums :photos` works without writing any Rust.

GraphQL column types map to typed Vantage columns: `int`, `bigint`, `float`, `bool`, `string`, `datetime`, `date`, `time`, `uuid`, `json`. Unknown types fail loudly at build time.

## Mixing Backends

Both adapters bridge into `AnyTable` through the shared `CborAdapter`, so a heterogeneous `Vec<AnyTable>` works without further wiring:

```rust
let rest = AnyTable::from_table(Table::<RestApi, EmptyEntity>::new(
    "users", RestApi::new("https://api.example.com"),
));
let graphql = AnyTable::from_table(Table::<GraphqlApi, EmptyEntity>::new(
    "launches", GraphqlApi::new("https://api.spacex.land/graphql"),
));

let backends: Vec<AnyTable> = vec![rest, graphql];
```

The same Vista bridge applies — `RestApi::vista_factory()` and `GraphqlApi::vista_factory()` both produce `Vista` instances whose `driver()` reports `"rest-api"` or `"graphql"` so generic UI code can branch on capability when needed.

## Relationship Traversal

`with_many` and `with_one` work on both adapters with the same semantics. For REST, a parent's `id=1` condition becomes either a URI substitution or a child query param. For GraphQL, the adapter peels an existing parent eq-condition into a direct child filter (the common `users(id=1) :albums` case) and falls back to async `DeferredField` resolution when the FK lives in the parent's data, not its conditions:

```rust
let mut users = Table::<RestApi, User>::new("users", api.clone())
    .with_id_column("id")
    .with_many("albums", "userId", Album::api_table_for_user);

users.add_condition(eq_condition("id", 1i64));
let albums = users.get_ref_as::<RestApi, Album>("albums")?;
// REST: GET /users/1/albums (URI template substitution)
// GraphQL: posts a single query with a `userId: 1` filter on `albums`
```

GraphQL relations are inherently two-round-trip in v1 — the parent fetch produces ids, the child fetch consumes them. Single-round-trip nested selection (rendering `launches { id rocket { id name } }` as one document) is on the roadmap; see the TODO list for status.

## Examples

The repository ships four end-to-end examples:

- **`jsonplaceholder`** — REST CLI over <https://jsonplaceholder.typicode.com>. User → Album → Photo traversal driven by typed Rust factories. URI templates show the nested-endpoint case.
- **`jsonplaceholder_yaml`** — same demo, schemas live in `data/jsonplaceholder/*.yaml` and load through `RestApiVistaFactory::register_yaml`.
- **`cities`** — two-level REST traversal (countries → cities) with auth headers.
- **`graphql_spacex`** — GraphQL CLI over the SpaceX public API at <https://spacex-api.fly.dev/graphql>. Ten entities (`launches`, `rockets`, `capsules`, `cores`, `ships`, `payloads`, `missions`, `dragons`, `landpads`, `launchpads`) defined in `examples/schema/*.yaml`. Run `cargo run --example graphql_spacex -- launches mission_name=FalconSat`. Endpoint overridable via `$SPACEX_ENDPOINT`.

## License

MIT OR Apache-2.0
