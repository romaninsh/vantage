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

```rust
let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(LAUNCHES_YAML)?;
let launches = api.vista_factory().build_from_spec(spec)?;
```

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
`contains` filter is reported as unimplemented, so the consumer filters locally.

### `filter: false`

This is for a root field that takes no filter argument. Conditions, including traversal
conditions, are applied to the fetched rows instead. Search, operators and paging switch off,
and counts list.

### Paging

With `paginate: true` and `filter` on, the Vista offers `fetch_page`, `fetch_window` and
`fetch_next`, sent as `$limit` / `$offset` variables. With `filter: false` a server page would
be cut short by the local filter, so paging is not offered. On Hasura, a paged read with no order
of its own is ordered by the id column so that pages stay stable. A Generic schema can't sort,
so its pages are only as stable as the server's natural order.

### Counting

`count: list` lists the rows and counts them, which works for every role. `count: aggregate`
(Hasura, with `filter` on) sends `<count_field>(where: …) { aggregate { count } }`. That needs a
role with `allow_aggregations`. If the server rejects the query, the count fails with an error
that suggests `count: list`; it never falls back silently. Generic tables always list.

### Search and ordering

A quicksearch ORs `_ilike` over the columns flagged `searchable`. If none are flagged, it uses
the string-typed columns other than the id. Dotted columns are skipped, and with no column left
the search matches nothing. `%`, `_` and `\` in the text are escaped. Ordering a dotted column is
refused.

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

`references:` takes `table`, `kind` (`has_one` / `has_many`) and `foreign_key`. Multi-column
`keys:` are refused at build. Traversal gives the target an eq-condition: `has_one` narrows the
target's id by the parent's `foreign_key`, and `has_many` narrows the target's `foreign_key` by
the parent's id. A text key is sent as an integer when the target column is an int.

A target is built from its own spec on the factory's API, without the parent table's
`graphql:` overrides. Targets come from the factory's registry (`register_yaml`, then
`build(name)`), or from a spec resolver:

```rust
let resolver: Arc<dyn SpecResolver<GraphqlApiVistaSpec>> =
    Arc::new(move |name: &str| specs.get(name).cloned());
let factory = api.vista_factory().with_resolver(resolver);
```

A reference to a model with no spec is an error naming the relation and the target, and no
request is sent.
