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
| `auth(..)` / `auth_refresher(..)` | none | `Authorization` header, fixed or refreshed after a `401` |
| `max_parallel`, `rate_limit`, `request_timeout`, `observer`, `http_client` | 4, none, 30 s, none, default | Transport; see the crate's `README_transport.md` |

A REST Vista can always count and traverse. It advertises `Window` when the API pages (a
`total_key` puts the grand total on every page), and `Order` when `ordering` is set. It never
advertises `Search`, so the Vista refuses a quicksearch rather than send one the API can't apply.
A Dio over it searches the rows it holds instead, over the columns flagged `searchable`, or every
column when none is flagged.
