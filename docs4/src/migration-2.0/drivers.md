# Drivers

The drivers' own factory APIs (`from_table`, `from_yaml`, `with_resolver`) keep their shape; what
changed is underneath and around them. Rule 1 removed the per-driver resolver aliases in favour of
one `SpecResolver<S>` trait, and rule 2 removed the places where a driver quietly ignored part of a
spec: `keys:` lists, column-level `references:`, and the `searchable` flag. Each now means the same
thing on every driver. The changes from the driver cleanup (PR #437) are on this page too, since
they land in the same release. Read [Config-Driven Vistas](../config-driven-vistas.md) and
[Adding a New Persistence](../new-persistence.md) for the current driver surface.

## The spec resolver is a trait

`SqliteSpecResolver`, `PostgresSpecResolver`, `MysqlSpecResolver`, `SurrealSpecResolver`,
`MongoSpecResolver` and `GraphqlSpecResolver` are gone. A factory holds
`Arc<dyn SpecResolver<XVistaSpec>>`, and any `Fn(&str) -> Option<S>` closure implements the trait.

```rust,compile_fail,E0432
# use std::collections::HashMap;
# use std::sync::Arc;
# use vantage_sql::prelude::*;
# use vantage_sql::sqlite::vista::SqliteVistaSpec;
use vantage_sql::sqlite::vista::SqliteSpecResolver;
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite::memory:").await.context("connect")?;
# let specs: HashMap<String, SqliteVistaSpec> = HashMap::from([("team".to_string(), SqliteVistaSpec::new("team"))]);

let resolver: SqliteSpecResolver = Arc::new(move |name| specs.get(name).cloned());
let factory = db.vista_factory().with_resolver(resolver);
# Ok(())
# }
```

```rust
# use std::collections::HashMap;
# use std::sync::Arc;
# use vantage_sql::prelude::*;
# use vantage_sql::sqlite::vista::SqliteVistaSpec;
use vantage_vista::SpecResolver;
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite::memory:").await.context("connect")?;
# let specs: HashMap<String, SqliteVistaSpec> = HashMap::from([("team".to_string(), SqliteVistaSpec::new("team"))]);

let resolver: Arc<dyn SpecResolver<SqliteVistaSpec>> =
    Arc::new(move |name: &str| specs.get(name).cloned());
let factory = db.vista_factory().with_resolver(resolver);
# let _ = factory;
# Ok(())
# }
```

Type the closure's parameter. `Arc::new(move |name| ..)` doesn't infer a parameter that takes
every lifetime, and fails to coerce with "implementation of `Fn` is not general enough". The same
holds for api-client's `ModelResolver`, now `with_model_resolver(Arc<dyn VistaResolver>)`, and for
any `|name: &str|` closure held as a `VistaResolver`.

### Why a trait and not the alias?

An alias for `Arc<dyn Fn(&str) -> Option<S>>` only admits closures. A trait admits a type of your
own, backed by a folder of YAML files or a hot-reloading inventory, with a `resolve` method you can
test. The cost is the parameter annotation above, which the alias inferred for you.

## The resolver rides on the handle

`RecordDraft::{new_row, load, from_row}` drop their resolver argument. `Handle::with_resolver` is
public, and a named handle resolves through the resolver it carries.

```rust,compile_fail,E0061
# use std::sync::Arc;
# use indexmap::IndexMap;
# use vantage_vista::rhai::{Handle, RecordDraft, Writes};
# use vantage_vista::{mocks::MockShell, prelude::*, Catalog};
# fn main() {
# let resolver: Arc<dyn VistaResolver> = Arc::new(Catalog::new().with("row", |_: &str| Ok(Vista::new("row", MockShell::new()))));
# let values: IndexMap<String, CborValue> = IndexMap::new();
let draft = RecordDraft::from_row(Handle::named("row"), Some(resolver), Writes::Allowed, "r1".to_string(), values);
# }
```

```rust
# use std::sync::Arc;
# use indexmap::IndexMap;
# use vantage_vista::rhai::{Handle, RecordDraft, Writes};
# use vantage_vista::{mocks::MockShell, prelude::*, Catalog};
# fn main() {
# let resolver: Arc<dyn VistaResolver> = Arc::new(Catalog::new().with("row", |_: &str| Ok(Vista::new("row", MockShell::new()))));
# let values: IndexMap<String, CborValue> = IndexMap::new();
let handle = Handle::named("row").with_resolver(resolver);
let draft = RecordDraft::from_row(handle, Writes::Allowed, "r1".to_string(), values);
# let _ = draft;
# }
```

A caller holding an `Option<Arc<dyn VistaResolver>>` attaches it only when present:
`match resolver { Some(r) => handle.with_resolver(r), None => handle }`.

## Specs mean the same on every driver

- **`keys:` is honoured.** A reference with a multi-column `keys:` list narrows on every pair. SQL,
  MongoDB and SurrealDB used to narrow on `foreign_key` alone and silently ignore the rest;
  vantage-memory used to refuse `keys:`. A spec without `keys:` lowers exactly as before.
- **Column-level `references:` reach every driver's metadata.** `client_id: { references: client }`
  is a has-one named after the column, with the column as its foreign key; the full form
  `{ table, kind, foreign_key }` keeps its own kind. A `references:` entry of the same name wins.
  Before, only the table registered it, so it was missing from the Vista's `references()` on
  MongoDB and named by the built table on SurrealDB. MongoDB now applies its "a key is stored under
  its own field name" check to these too.
- **`Reference.target` is the spec name** (`table:`), even when the spec reads another physical
  table, and a typed relation's target is its built table's name, so `catalog.vista(&r.target)`
  works everywhere.

### Declare the foreign key on the target

Traversal narrows the target by the join's `to` column, so the target has to declare it. A
has-many target that leaves out its foreign key column fails the traversal with an error naming
the relation and the column, where 1.x injected or ignored it:

```yaml
name: product
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
  bakery_id: { type: string }   # the has-many `bakery.products` narrows by this
```

## Quicksearch reads the searchable columns

Search reads the columns flagged `Searchable`, or every column when none is. The evaluator,
`MockShell` and every driver follow the rule through `VistaMetadata::search_columns` (vista) and
`Table::search_columns` (vantage-table). A SQL table with only `name` flagged no longer searches
every column; a MongoDB spec with no flag now searches every column, where it matched nothing.

```rust
use vantage_vista::{Column, ColumnFlag, VistaMetadata};

let metadata = VistaMetadata::new()
    .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
    .with_column(Column::new("name", "String").with_flag(ColumnFlag::Searchable))
    .with_column(Column::new("notes", "String"))
    .with_id_column("id");
assert_eq!(metadata.search_columns(), ["name"]);
```

GraphQL searches the flagged columns, or every text column but the id when none is flagged.
Kubernetes drops `searchable_titles` and searches every declared column. `MemoryCondition::Search(t)`
is `MemoryCondition::search(t)`, building a `Search { text, columns }` that reads every cell.

## Any window from a listing

A shell that can only list builds its window with `Page::from_listing(rows, offset, limit)`: any
offset, the listing's length as the total. It replaces csv's, cmd's and kubernetes' own slicing and
the interim `Page::leading_window`. `Vista` still refuses an offset above zero without `Window`
before the shell is called.

```rust,compile_fail,E0599
# use vantage_types::Record;
# use vantage_vista::{CborValue, Page};
# fn main() -> vantage_core::Result<()> {
# let listing: Vec<(String, Record<CborValue>)> = ["a", "b", "c"].iter().map(|id| (id.to_string(), Record::new())).collect();
let page = Page::leading_window(listing, 0, 2)?;
# Ok(())
# }
```

```rust
# use vantage_types::Record;
# use vantage_vista::{CborValue, Page};
# fn main() -> vantage_core::Result<()> {
# let listing: Vec<(String, Record<CborValue>)> = ["a", "b", "c"].iter().map(|id| (id.to_string(), Record::new())).collect();
let page = Page::from_listing(listing, 2, 2);
# assert_eq!((page.rows.len(), page.total), (1, Some(3)));
# Ok(())
# }
```

## Renames

| 1.x | 2.0 |
|---|---|
| `vantage_memory::vista::Catalog`, `Catalog::shell(n)`, `Catalog::get(n)` | `MemoryCatalog`, a `VistaResolver`: `catalog.vista(n)` (`is_not_found()` for an unknown name) |
| `MemoryTableShell::with_untyped_columns(cols)`, `capabilities_for_memory()` | `Column::untyped(name)`; `vista.capabilities()` |
| vantage-table `Reference::cardinality() -> Cardinality`, `foreign_key()` | `Reference::vista_reference(name, target, source_id) -> vantage_vista::Reference` |
| `Table::with_spec_references(&spec.references, ..)` | `with_spec_references(&spec, ..)`; `table::spec::spec_references(spec, source_id)` for metadata |
| `Table::name_join_targets`, `name_spec_targets`, `Reference::target_type_name()` | removed; `target_table_name(data_source)` |
| `ContainedRelation::new(name, host, kind, id_column, build)` | `ContainedRelation::one(name, host, build)`, `many(name, host, id_column, build)` |
| `get_contained_ref(rel, row, parent_id, ..)` | `get_contained_ref(rel, Option<(&row, id)>, ..)`; `None` is the bare target |
| `MockReference::new(cardinality, fk)` (public) | test-only |
| serialised `Preview` `{driver, query, note}` | `{query, note}`; add `driver` from `vista.driver_name()` yourself |
| faker shape YAML capabilities `fetch_next`, `fetch_window`, `count`, `fetch_page` | `cursor`, `window`; `count` and `fetch_page` removed |
| request-parameter columns flagged `hidden` | `flags: [hidden, request_param]` (`ColumnFlag::RequestParam`) |
| `temp_remove_condition(h)` then `temp_add_condition(c)` | `replace_temp_condition(old, new)`; removing a stale handle is `is_not_found()` |
| `build_contained_vista(.., writeback, ..)` | `writeback: Option<ContainedWriteback>`; `None` builds the bare target |
| `register_constructors!(engine, value: V)` and the other vantage-sql macros | `register_constructors!(engine, ns, value: V)`; `register_engine!` is unchanged |

## Behaviour notes

- **Dotted conditions.** SQL refuses a dotted or imported column ("can't be filtered in SQL");
  MongoDB and SurrealDB evaluate `root.leaf` natively when `root` is declared; GraphQL with
  push-down refuses an undeclared dotted path in `add_condition`.
- **SurrealDB** re-tags a record id held as text on the id column and on record-link columns (typed
  `thing`/`record`, or read by one of the table's own has-one references) for every caller; any
  other column compares text as text.
- **GraphQL** types each text operand by its column, so an integer column takes `_eq: 5`, and a
  non-integer text on one is an error before any request.
- **REST, GraphQL and AWS** answer `fetch_window(0, n)` from one unpaged listing.
- **AWS and cmd** apply request-parameter prefix filters (`PathPrefix`, S3 `prefix`,
  `logGroupNamePrefix`) to typed tables only; a YAML vista over them doesn't.
- **REST, AWS and cmd client-side filters** pass a row lacking the field only for a
  `request_param` column, a `{placeholder}` or an undeclared field; a `hidden` column is a row
  field.
- **cmd** answers `get(id)` only with the row keyed by `id`, or a single id-less row; on a narrowed
  set, `get` returns nothing for ids outside it.
- **vantage-memory and faker** refuse a condition on a column the vista doesn't declare. Memory
  conforms seeded cells to their declared type and creates no table for an unknown traversal
  target. Faker counts a write only when it changed a row, and sims chain their handle calls.
- **spacetimedb** refuses `IN`, `NOT IN` and `LIKE` (evaluated locally), advertises no writes on a
  keyless table, refuses writes through a narrowed handle and serves only the first window.
- **REST** conditions accept declared columns and endpoint placeholders only; a non-scalar `Eq`
  value is refused, so it is evaluated locally.
- **YAML flags reach the typed column.** `searchable`, `mandatory`, `label`, `indexed`, `hidden` and
  `request_param` all carry through `Column::from_spec`, and drivers with column flags (MongoDB,
  SurrealDB, GraphQL, SQL) show them on the Vista column.
- **Unknown relations** are `vantage_vista::unknown_relation` on every driver: "unknown relation",
  `is_not_found()`.

## Checklist

- [ ] no `*SpecResolver` alias; `Arc<dyn SpecResolver<XVistaSpec>>` with `|name: &str|` closures
- [ ] `RecordDraft` calls carry the resolver on the handle: `Handle::named(..).with_resolver(r)`
- [ ] specs with `keys:` or column `references:` are checked against the traversals they now drive
- [ ] has-many targets declare their foreign key column
- [ ] the columns a search should read are flagged `searchable`
- [ ] custom shells window a listing with `Page::from_listing`
