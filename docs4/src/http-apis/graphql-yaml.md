# GraphQL: the `graphql:` blocks

A GraphQL spec is a `GraphqlApiVistaSpec`. It has a table-level `graphql:` block for the root
field and what the server accepts, and an optional column-level `graphql:` block for a column
whose server field has a different name.

```yaml
name: launches
id_column: id
columns:
  id: { type: string, flags: [id] }
  mission: { type: string, flags: [title, searchable], graphql: { field: mission_name } }
  launch_year: { type: int }
  rocket_id: { type: string }
  rocket.rocket_name: { type: string }
references:
  rocket: { table: rockets, kind: has_one, foreign_key: rocket_id }
graphql:
  root_field: launches
  dialect: hasura
  paginate: true
  count: aggregate
```

```rust,no_run
# use vantage_api_client::{GraphqlApi, GraphqlApiVistaSpec};
# use vantage_vista::VistaFactory;
# const LAUNCHES_YAML: &str = r#"
# name: launches
# id_column: id
# columns:
#   id: { type: string, flags: [id] }
#   mission: { type: string, flags: [title, searchable], graphql: { field: mission_name } }
#   launch_year: { type: int }
#   rocket_id: { type: string }
#   rocket.rocket_name: { type: string }
# references:
#   rocket: { table: rockets, kind: has_one, foreign_key: rocket_id }
# graphql:
#   root_field: launches
#   dialect: hasura
#   paginate: true
#   count: aggregate
# "#;
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let api = GraphqlApi::new("https://hasura.example.com/v1/graphql");
let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(LAUNCHES_YAML)?;
let launches = api.vista_factory().build_from_spec(spec)?;
# let _ = launches;
# Ok(())
# }
```

`build_from_spec` comes from the [`VistaFactory`](vantage_vista::VistaFactory) trait, so it needs
`use vantage_vista::VistaFactory;`. The block is compiled but not run: building the API makes a
`reqwest` client, which reads the system's proxy settings.

## Table keys

| Key | Type | Default | Meaning |
|---|---|---|---|
| `root_field` | name | the spec `name` | Query field the rows are read from |
| `dialect` | `generic` \| `hasura` | the API's dialect | How filters are spelled; lowercase only |
| `filter_arg` | name | the API's, else `where` (Hasura) / `find` (Generic) | Name of the filter argument |
| `args` | map | the API's `root_args` | Literal arguments always passed to the root field |
| `response_path` | dotted path | the API's | Path walked into the root field's value to reach the rows (`edges.node`) |
| `filter` | bool | `true` | Conditions may be sent to the server |
| `order` | bool | `true` on Hasura, else `false` | `order_by:` may be sent |
| `search` | bool | `true` on Hasura, else `false` | Quicksearch may be sent as `_ilike`s; never with `filter: false` |
| `paginate` | bool | `false` | `limit:` / `offset:` may be sent |
| `count` | `list` \| `aggregate` | `list` | How `get_count` is answered |
| `count_field` | name | the API's, else `<root_field>_aggregate` | Aggregate field for `count: aggregate` |

The whole block is optional, and unknown keys or values are a parse error. Every name the block
puts into a query is checked against the GraphQL name grammar (`[_A-Za-z][_0-9A-Za-z]*`, dotted
paths segment by segment) when the Vista is built.

The block's `filter`, `order`, `search`, `paginate` and `count` override the API's `Supports` for
that table, key by key. A key left out keeps the builder's setting, and the default in the table
above applies only when the builder left it unset too.

### Dialects

| Dialect | Filter | Operators |
|---|---|---|
| `hasura` | `launches(where: {launch_year: {_eq: 2018}})` | the full set, plus `_and` / `_or` / `_not` |
| `generic` | `launches(find: {launch_year: 2018})` | equality only |

A condition the dialect can't spell fails before anything is sent. On Generic, a Vista
`contains` filter is refused as unsupported, so the consumer filters locally.

### `filter: false`

This is for a root field that takes no filter argument. Conditions, including traversal
conditions, are applied to the fetched rows instead. Search, operators and paging switch off,
and counts list.

### Paging

With `paginate: true` and `filter` on, the Vista advertises `Window` and serves `fetch_window`,
sent as `$limit` / `$offset` variables. It pages by offset only, so it never advertises `Cursor`.
With `filter: false` a server page would be cut short by the local filter, so paging is not
offered. On Hasura, a paged read with no order
of its own is ordered by the id column so that pages stay stable. A Generic schema can't sort,
so its pages are only as stable as the server's natural order.

### Counting

`count: list` lists the rows and counts them, which works for every role. `count: aggregate`
(Hasura, with `filter` on) sends `<count_field>(where: …) { aggregate { count } }`. That needs a
role with `allow_aggregations`. If the server rejects the query, the count fails with an error
that suggests `count: list`; it never falls back silently. Generic tables always list.

### Search and ordering

A quicksearch ORs `_ilike` over the columns every driver searches: those flagged `searchable`, or
every column when none is flagged (`Table::search_columns`). `_ilike` takes text, so when nothing
is flagged GraphQL keeps only the string-typed columns other than the id. Dotted columns are then
skipped. The rule is settled before they are, so a table whose only searchable columns are dotted
matches nothing rather than widening to every string column. With no column left the search
matches nothing. `%`, `_` and `\` in the text are escaped. Ordering a dotted column is refused.

## Column keys

| Key | Type | Default | Meaning |
|---|---|---|---|
| `graphql.field` | name | the column name | Server field the column reads |

The query selects an aliased column as `mission: mission_name`, so rows keep the column name.
Conditions, sort keys and search on the column are sent as the server field. Both names must be
plain (undotted).

A dotted column (`rocket.rocket_name`) renders as a nested selection, `rocket { rocket_name }`.
A `lazy:` column is computed from each fetched row and never selected (crate feature `rhai`).

| `type` | Column |
|---|---|
| `int`, `integer`, `i32` | `i32` |
| `bigint`, `i64`, `long` | `i64` |
| `float`, `double`, `f64`, `f32` | `f64` |
| `bool`, `boolean` | `bool` |
| `string`, `text`, `str`, `id` (default `string`) | `String` |
| `datetime`, `timestamp`, `timestamptz` | `DateTime<Utc>` |
| `date`, `time` | `NaiveDate`, `NaiveTime` |
| `uuid` | `Uuid` |
| `json` | `serde_json::Value` |

Get-by-id sends an integer literal for an int id column, and a quoted string otherwise. A row
whose id is null or empty is keyed `~row:<position>`. Two rows with the same id in one
response are an error.

## References

A relation is declared in one of two places:

- a `references:` entry: `table`, `kind` (`has_one` / `has_many`) and `foreign_key`;
- a column-level `references:`, for a has-one keyed by the column itself.
  `rocket_id: { type: string, references: rockets }` is a has-one named `rocket_id`, joining
  `rocket_id` to the id of `rockets`. An entry of the same name in the `references:` map wins.

Multi-column `keys:` on a `references:` entry are refused when the Vista is built, with an
unsupported error naming the relation. A GraphQL filter argument narrows by one field per relation
here, so the driver refuses a join it would only half apply.

Traversal gives the target an eq-condition: `has_one` narrows the target's id by the parent's
`foreign_key`, and `has_many` narrows the target's `foreign_key` by the parent's id. A text key is
sent as an integer when the target column is an int.

A target is built from its own spec on the factory's API, without the parent table's
`graphql:` overrides. Targets come from the factory's registry (`register_yaml`, then
`build(name)`), or from a spec resolver:

```rust,no_run
# use std::collections::HashMap;
# use std::sync::Arc;
# use vantage_api_client::{GraphqlApi, GraphqlApiVistaSpec};
# use vantage_vista::SpecResolver;
# let api = GraphqlApi::new("https://hasura.example.com/v1/graphql");
# let specs: HashMap<String, GraphqlApiVistaSpec> = HashMap::new();
let resolver: Arc<dyn SpecResolver<GraphqlApiVistaSpec>> =
    Arc::new(move |name: &str| specs.get(name).cloned());
let factory = api.vista_factory().with_resolver(resolver);
```

Any `Fn(&str) -> Option<GraphqlApiVistaSpec>` closure is a
[`SpecResolver`](vantage_vista::SpecResolver); a spec is looked up each time a traversal builds its
target, so the target always reflects the current spec.

A reference to a model with no spec is an error naming the relation and the target, and no
request is sent.
