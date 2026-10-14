# Step 8: Vista Integration

By the end of Step 6 your backend is a typed persistence: `Table<T, E>`, conditions, relationships.
That's what business logic wants, and it's exactly what generic code can't use.

A CLI that lists "any table from any backend" can't carry an entity generic. A web admin that draws
forms from a YAML schema doesn't know your `Product` struct exists. A Rhai script that filters rows
by a column name shouldn't compile against your backend's condition type. **Vista** is the bridge: a
schema-bearing handle over one table of one backend, with CBOR values and string ids at its surface.

This step adds two pieces to your backend: a **shell** that executes reads, narrowing and writes for
a Vista, and a **factory** that builds a Vista from a typed `Table` or a YAML spec.

### What Vista is

[`Vista`](vantage_vista::Vista) is a concrete struct, not a trait. It holds a name, the clauses
applied to it (a [`VistaQuery`](vantage_vista::VistaQuery)) and a boxed
[`TableShell`](vantage_vista::TableShell): your driver's executor. The schema and the capabilities
live on the shell. `Vista` reads them from there and checks every call against them before the
shell sees it.

A `Vista` is a value. `vista.clone().with_condition(..)` gives a narrowed set and leaves `vista`
alone, and it does that by cloning the shell and narrowing the clone. That's why `TableShell` has
`Clone` as a supertrait: a shell's clone must carry its current narrowing and own it, while sharing
the connection or the store.

### Why not hand around `Table<T, E>`?

You can, and wherever the entity is known at compile time you should: it's typed, it composes, and
it's what the previous steps built. Vista is for the cases where the entity isn't known at compile
time, or the backend is chosen at runtime:

- a CLI driven by `--source surreal --table client list`, with no `Client` struct in scope;
- an admin tool that reads its schema from YAML;
- a Rhai callback that filters rows by a column name.

The price of erasure is that values become `CborValue` and ids become `String`. Generic code renders
values; it doesn't deserialize them into structs, so it doesn't miss the types.

### Setting it up

The bridge is opt-in behind a `vista` feature, so a crate that only uses `Table` doesn't pull in
`vantage-vista`. The contract checks used in the tests come from vantage-vista's `contract`
feature, enabled for tests only:

```toml
[features]
default = []
vista = ["dep:vantage-vista"]

[dependencies]
vantage-vista = { path = "../vantage-vista", optional = true }

[dev-dependencies]
vantage-vista = { path = "../vantage-vista", features = ["contract"] }
```

Everything Vista-related sits under `#[cfg(feature = "vista")]`. CSV's layout is the one to copy:

```text
<driver>/src/vista.rs          re-exports, and <Driver>::vista_factory()
<driver>/src/vista/spec.rs     <Driver>TableExtras, <Driver>ColumnExtras, <Driver>VistaSpec
<driver>/src/vista/factory.rs  <Driver>VistaFactory and its impl VistaFactory
<driver>/src/vista/source.rs   <Driver>TableShell and its impl TableShell
```

A backend whose native values aren't CBOR-shaped also needs a bridge between the two. MongoDB keeps
its BSON ↔ CBOR conversion in `types/cbor.rs`.

#### The shell

Here is a complete shell over rows held in memory. It's the smallest backend there is, so
everything in it is the trait's shape rather than a driver's detail. It evaluates every clause with
the same evaluator a cache uses ([`VistaQuery::matches`](vantage_vista::VistaQuery::matches) and
[`VistaQuery::sort_rows`](vantage_vista::VistaQuery::sort_rows)), so it can advertise operator
filters, search and order:

```rust
use std::sync::Arc;

use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{
    Capability, CborValue, Column, ColumnFlag, Condition, FilterOp, Page, Preview, SortDirection,
    TableShell, Vista, VistaCapabilities, VistaMetadata, VistaQuery, condition_column_error,
};

type Rec = Record<CborValue>;

/// Rows held in memory. A clone shares the rows and owns its narrowing.
#[derive(Clone)]
struct VecShell {
    rows: Arc<Vec<(String, Rec)>>,
    query: VistaQuery,
    metadata: VistaMetadata,
    capabilities: VistaCapabilities,
}

impl VecShell {
    fn new(rows: Arc<Vec<(String, Rec)>>, metadata: VistaMetadata) -> Self {
        let capabilities = [
            Capability::FilterOps,
            Capability::Search,
            Capability::Order,
            Capability::Window,
        ]
        .into_iter()
        .collect();
        Self { rows, query: VistaQuery::new(), metadata, capabilities }
    }

    /// The rows the narrowing keeps, in its order.
    fn set(&self) -> Result<Vec<(String, Rec)>> {
        let mut rows = Vec::new();
        for (id, row) in self.rows.iter() {
            if self.query.matches(row, &self.metadata)? {
                rows.push((id.clone(), row.clone()));
            }
        }
        self.query.sort_rows(&mut rows, &self.metadata);
        Ok(rows)
    }

    /// A declared column, or a dotted path into one.
    fn check_column(&self, column: &str) -> Result<()> {
        let root = column.split('.').next().unwrap_or(column);
        let columns = &self.metadata.columns;
        if columns.contains_key(column) || columns.contains_key(root) {
            Ok(())
        } else {
            Err(condition_column_error(&self.metadata, column))
        }
    }
}

#[async_trait]
impl TableShell for VecShell {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "vec"
    }

    fn preview(&self) -> Preview {
        Preview::new().with_note("every clause is evaluated in memory")
    }

    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        Ok(self.set()?.into_iter().collect())
    }

    async fn get(&self, id: &str) -> Result<Option<Rec>> {
        Ok(self.set()?.into_iter().find(|(key, _)| key == id).map(|(_, row)| row))
    }

    async fn count(&self) -> Result<i64> {
        Ok(self.set()?.len() as i64)
    }

    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        Ok(Page::from_listing(self.set()?, offset, limit))
    }

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.check_column(&condition.column)?;
        self.query.conditions.push(condition.clone());
        Ok(())
    }

    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.query.search = text.map(str::to_string);
        Ok(())
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        if let Some((column, _)) = order {
            self.check_column(column)?;
        }
        self.query.order = order.map(|(column, dir)| (column.to_string(), dir));
        Ok(())
    }
}

fn row(id: &str, name: &str, category: &str) -> (String, Rec) {
    let cells = [("id", id), ("name", name), ("category", category)];
    let record = cells.into_iter().map(|(k, v)| (k.to_string(), CborValue::from(v))).collect();
    (id.to_string(), record)
}

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let metadata = VistaMetadata::new()
    .with_column(Column::new("id", "string").with_flag(ColumnFlag::Orderable))
    .with_column(Column::new("name", "string").with_flag(ColumnFlag::Searchable))
    .with_column(Column::new("category", "string"))
    .with_id_column("id");
let rows = Arc::new(vec![
    row("p1", "Tea", "drink"),
    row("p2", "Cake", "food"),
    row("p3", "Coffee", "drink"),
]);
let products = Vista::new("product", VecShell::new(rows, metadata));

let drinks = products.clone().with_condition("category", FilterOp::Eq, "drink")?;
assert_eq!(drinks.count().await?, 2);
assert_eq!(products.count().await?, 3);
# Ok(())
# }
```

The methods split into what every shell answers and what it opts into.

**Every shell** answers `metadata`, `capabilities`, `driver_name`, `preview`, `list`, `get`, `count`,
`fetch_window` and `add_condition` with equality.

- `metadata` is the schema. A shell over a typed table doesn't build it by hand: CSV's factory asks
  the table for it with `table.vista_metadata(VistaMetadataOptions { references: true, .. })`, which
  carries the column types, the id and title columns and the relations.
- `fetch_window` with `offset == 0` is how a capped read and `get_some_value` read, so every shell
  that lists must answer it. A backend that can only list cuts the window from its listing with
  [`Page::from_listing`](vantage_vista::Page::from_listing), which also fills the page's total. A
  backend that pages natively pushes `LIMIT`/`OFFSET` and sets the total only when the fetch learned
  it (`Page::new(rows).with_total(n)`).
- `add_condition` refuses a column the shell doesn't take with
  [`condition_column_error`](vantage_vista::condition_column_error). That gives a plain
  unknown-column error for a column the schema doesn't declare, and an `Unimplemented` refusal for a
  dotted path the shell can't evaluate. The kinds matter: `Vista::with_query` hands a refused clause
  back to the caller to evaluate locally, and passes a plain error on. A shell that errors must leave
  its narrowing unchanged, which is why `VecShell` checks before it pushes.
- `preview` renders the query the next read would send, without sending it. Lossy is fine, wrong is
  not: it must not show a clause the shell wouldn't send.

**A shell opts into** the rest by implementing the method and advertising its
[`Capability`](vantage_vista::Capability). Each default answers
[`unsupported`](vantage_vista::unsupported) with the capability that names it:

| Capability         | Methods                                            |
| ------------------ | -------------------------------------------------- |
| `Insert`           | `insert`                                           |
| `Update`           | `replace`, `upsert`, `patch`                       |
| `Delete`           | `delete`, `delete_all`                             |
| `Import`           | `import`, one atomic batch that counts new rows    |
| `Subscribe`        | `watch`                                            |
| `Order`            | `set_order`                                        |
| `Search`           | `set_search`                                       |
| `FilterOps`        | `add_condition` with any operator other than `Eq`  |
| `NativeConditions` | `add_native_condition`                             |
| `Window`           | `fetch_window` at any offset                       |
| `Cursor`           | `fetch_cursor`                                     |
| `Traverse`         | `get_ref`                                          |
| `ConfineWrites`    | every write keeps to the narrowed set              |

`Vista` refuses most of these before the shell is called: writes, search, order, operator filters,
windows past the first, cursors and subscriptions. `get_ref` and `add_native_condition` reach the
shell, and the default refuses them there. CSV pushes only equality, so it doesn't advertise
`FilterOps`, and a `Gt` never reaches it. Its `add_condition` still answers
`unsupported(Capability::FilterOps)` for any other operator, so the shell is correct on its own.

Two of the opt-in methods need more than the table above says:

- `get_ref(relation, row)` is the one traversal method. With a parent row it returns the related
  rows; with `None` it returns the bare target, which is what an edit form lists to pick a related
  record from. A shell over a typed table forwards both to
  [`Table::traverse_erased`](vantage_table::table::Table::traverse_erased), passing its own factory
  to wrap the target, and answers an undeclared relation with
  [`unknown_relation`](vantage_vista::unknown_relation). CSV's `get_ref` looks the reference up in
  its metadata and makes that one call. Contained relations route through the same method (Step 9).
- `add_native_condition` takes a [`NativeCondition`](vantage_vista::NativeCondition): a condition
  in your driver's own type, passed through Vista unread. Downcast it to the type you take. Another
  type is the caller's mistake, so it's an `IncorrectUsage` error, not `Unsupported`.

#### The factory

The factory is where both construction paths meet. Expose it as an inherent `vista_factory()` on the
data source, so callers don't name an extra type. It carries two entry points:

- `from_table<E>(table: Table<YourDB, E>) -> Result<Vista>`, inherent, because putting it on the
  trait would make vantage-vista depend on vantage-table;
- `impl VistaFactory`, whose `build_from_spec` lowers a parsed YAML spec. The default `from_yaml`
  parses the YAML and calls it.

Keep them on one path. CSV's `build_from_spec` builds a `Table<Csv, EmptyEntity>` from the spec,
hands it to `from_table`, and renames the result with `with_name(spec.name)`, so a spec called
`client` over a file `clients.csv` shows as `client`. A capability added later is then a one-line
edit in one place, and the two paths can't drift apart.

For the memory shell above there is no typed table, so the factory lowers the spec straight into
metadata:

```rust
# use std::sync::Arc;
# use async_trait::async_trait;
# use indexmap::IndexMap;
# use vantage_core::Result;
# use vantage_types::Record;
# use vantage_vista::{
#     Capability, CborValue, Column, Condition, Page, Preview, SortDirection, TableShell, Vista,
#     VistaCapabilities, VistaMetadata, VistaQuery, condition_column_error,
# };
# type Rec = Record<CborValue>;
# #[derive(Clone)]
# struct VecShell {
#     rows: Arc<Vec<(String, Rec)>>,
#     query: VistaQuery,
#     metadata: VistaMetadata,
#     capabilities: VistaCapabilities,
# }
# impl VecShell {
#     fn new(rows: Arc<Vec<(String, Rec)>>, metadata: VistaMetadata) -> Self {
#         let capabilities = [
#             Capability::FilterOps,
#             Capability::Search,
#             Capability::Order,
#             Capability::Window,
#         ]
#         .into_iter()
#         .collect();
#         Self { rows, query: VistaQuery::new(), metadata, capabilities }
#     }
#     fn set(&self) -> Result<Vec<(String, Rec)>> {
#         let mut rows = Vec::new();
#         for (id, row) in self.rows.iter() {
#             if self.query.matches(row, &self.metadata)? {
#                 rows.push((id.clone(), row.clone()));
#             }
#         }
#         self.query.sort_rows(&mut rows, &self.metadata);
#         Ok(rows)
#     }
#     fn check_column(&self, column: &str) -> Result<()> {
#         let root = column.split('.').next().unwrap_or(column);
#         let columns = &self.metadata.columns;
#         if columns.contains_key(column) || columns.contains_key(root) {
#             Ok(())
#         } else {
#             Err(condition_column_error(&self.metadata, column))
#         }
#     }
# }
# #[async_trait]
# impl TableShell for VecShell {
#     fn metadata(&self) -> &VistaMetadata {
#         &self.metadata
#     }
#     fn capabilities(&self) -> &VistaCapabilities {
#         &self.capabilities
#     }
#     fn driver_name(&self) -> &'static str {
#         "vec"
#     }
#     fn preview(&self) -> Preview {
#         Preview::new().with_note("every clause is evaluated in memory")
#     }
#     async fn list(&self) -> Result<IndexMap<String, Rec>> {
#         Ok(self.set()?.into_iter().collect())
#     }
#     async fn get(&self, id: &str) -> Result<Option<Rec>> {
#         Ok(self.set()?.into_iter().find(|(key, _)| key == id).map(|(_, row)| row))
#     }
#     async fn count(&self) -> Result<i64> {
#         Ok(self.set()?.len() as i64)
#     }
#     async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
#         Ok(Page::from_listing(self.set()?, offset, limit))
#     }
#     fn add_condition(&mut self, condition: &Condition) -> Result<()> {
#         self.check_column(&condition.column)?;
#         self.query.conditions.push(condition.clone());
#         Ok(())
#     }
#     fn set_search(&mut self, text: Option<&str>) -> Result<()> {
#         self.query.search = text.map(str::to_string);
#         Ok(())
#     }
#     fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
#         if let Some((column, _)) = order {
#             self.check_column(column)?;
#         }
#         self.query.order = order.map(|(column, dir)| (column.to_string(), dir));
#         Ok(())
#     }
# }
# fn row(id: &str, name: &str, category: &str) -> (String, Rec) {
#     let cells = [("id", id), ("name", name), ("category", category)];
#     let record = cells.into_iter().map(|(k, v)| (k.to_string(), CborValue::from(v))).collect();
#     (id.to_string(), record)
# }
use vantage_vista::{NoExtras, VistaFactory, VistaSpec};

struct VecVistaFactory {
    rows: Arc<Vec<(String, Rec)>>,
}

impl VistaFactory for VecVistaFactory {
    type TableExtras = NoExtras;
    type ColumnExtras = NoExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: VistaSpec) -> Result<Vista> {
        let mut metadata = VistaMetadata::new().with_id_column(spec.resolve_id_column("id"));
        for (name, column) in &spec.columns {
            let declared = match &column.col_type {
                Some(ty) => Column::new(name, ty),
                None => Column::untyped(name),
            };
            let declared = column.flags.iter().cloned().fold(declared, Column::with_flag);
            metadata = metadata.with_column(declared);
        }
        Ok(Vista::new(spec.name, VecShell::new(self.rows.clone(), metadata)))
    }
}

const PRODUCT: &str = r#"
name: product
columns:
  id: { type: string, flags: [id, orderable] }
  name: { type: string, flags: [title, searchable] }
  category: { type: string }
"#;

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let rows = vec![row("p1", "Tea", "drink"), row("p2", "Cake", "food"), row("p3", "Coffee", "drink")];
let factory = VecVistaFactory { rows: Arc::new(rows) };
let products = factory.from_yaml(PRODUCT)?;
assert_eq!(products.id_column(), Some("id"));
assert_eq!(products.title_columns(), ["name"]);
# Ok(())
# }
```

Three spec rules hold for every driver:

- **Resolve the id column in a fixed order**: `id_column:` first, then the first column flagged
  `id`, then the backend's own default (`"id"` for SQL, `"_id"` for MongoDB).
  `VistaSpec::resolve_id_column` does exactly that. Don't reverse it: an explicit `id_column:` is how
  a YAML author corrects a bad flag without editing the schema source.
- **Driver blocks reject typos.** Driver-specific YAML lives under the three associated types,
  conventionally under a key named after the driver (`csv: { path }`, `mongo: { collection }`). Put
  `#[serde(deny_unknown_fields)]` on every extras struct, or `mongo: { collctiom: clients }` falls
  back to defaults and you debug a missing collection at runtime instead of at parse time. Make the
  fields `Option<T>` with `#[serde(default)]` where the spec name is a sensible fallback.
- **Validate up front.** Reject bad paths and conflicting options in `build_from_spec`, with the
  column name in the error, so the YAML author can find the entry.

#### The boundary

- **Ids stringify.** MongoDB's `ObjectId` becomes its hex form, SurrealDB's `Thing` its `table:id`
  form. Inside the shell you parse the string back. Round-trip it in a test: `get` must find a row by
  the very string `list` returned for it.
- **Values become CBOR.** On the way in, unwrap `CborValue::Tag(_, inner)` and store the inner
  value. Stringify integers wider than your native type rather than truncating them. Keep the bridge
  module-private: a public BSON ↔ CBOR conversion gets called from places that should go through a
  Vista instead.

### Why must every advertised capability work?

Because a capability is the only thing a generic caller checks. A UI draws an "add" button when the
shell advertises `Insert`, a grid offers sorting when it advertises `Order`, and a cache pushes a
filter down instead of evaluating it itself when it advertises `FilterOps`. Nothing else warns the
caller, so a flag the shell can't honour turns a clean "not supported" into a failure in front of a
user.

`vantage_vista::contract` makes that a test failure instead. `check_advertised` calls the method
behind each advertised capability on your fixture, and fails when one refuses, errors on a valid
fixture, or does nothing: a search that counts as many rows as before, an ascending and a
descending order that list the same row first, a `Gt` that keeps its own pivot row. It also holds
every shell to the leading window and to the dotted-path rule above. `check_clone` checks that a
narrowed clone leaves the original alone. Each driver runs both against its own fixture:

```rust
# use std::sync::Arc;
# use async_trait::async_trait;
# use indexmap::IndexMap;
# use vantage_core::Result;
# use vantage_types::Record;
# use vantage_vista::{
#     Capability, CborValue, Column, ColumnFlag, Condition, Page, Preview, SortDirection,
#     TableShell, Vista, VistaCapabilities, VistaMetadata, VistaQuery, condition_column_error,
# };
# type Rec = Record<CborValue>;
# #[derive(Clone)]
# struct VecShell {
#     rows: Arc<Vec<(String, Rec)>>,
#     query: VistaQuery,
#     metadata: VistaMetadata,
#     capabilities: VistaCapabilities,
# }
# impl VecShell {
#     fn new(rows: Arc<Vec<(String, Rec)>>, metadata: VistaMetadata) -> Self {
#         let capabilities = [
#             Capability::FilterOps,
#             Capability::Search,
#             Capability::Order,
#             Capability::Window,
#         ]
#         .into_iter()
#         .collect();
#         Self { rows, query: VistaQuery::new(), metadata, capabilities }
#     }
#     fn set(&self) -> Result<Vec<(String, Rec)>> {
#         let mut rows = Vec::new();
#         for (id, row) in self.rows.iter() {
#             if self.query.matches(row, &self.metadata)? {
#                 rows.push((id.clone(), row.clone()));
#             }
#         }
#         self.query.sort_rows(&mut rows, &self.metadata);
#         Ok(rows)
#     }
#     fn check_column(&self, column: &str) -> Result<()> {
#         let root = column.split('.').next().unwrap_or(column);
#         let columns = &self.metadata.columns;
#         if columns.contains_key(column) || columns.contains_key(root) {
#             Ok(())
#         } else {
#             Err(condition_column_error(&self.metadata, column))
#         }
#     }
# }
# #[async_trait]
# impl TableShell for VecShell {
#     fn metadata(&self) -> &VistaMetadata {
#         &self.metadata
#     }
#     fn capabilities(&self) -> &VistaCapabilities {
#         &self.capabilities
#     }
#     fn driver_name(&self) -> &'static str {
#         "vec"
#     }
#     fn preview(&self) -> Preview {
#         Preview::new().with_note("every clause is evaluated in memory")
#     }
#     async fn list(&self) -> Result<IndexMap<String, Rec>> {
#         Ok(self.set()?.into_iter().collect())
#     }
#     async fn get(&self, id: &str) -> Result<Option<Rec>> {
#         Ok(self.set()?.into_iter().find(|(key, _)| key == id).map(|(_, row)| row))
#     }
#     async fn count(&self) -> Result<i64> {
#         Ok(self.set()?.len() as i64)
#     }
#     async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
#         Ok(Page::from_listing(self.set()?, offset, limit))
#     }
#     fn add_condition(&mut self, condition: &Condition) -> Result<()> {
#         self.check_column(&condition.column)?;
#         self.query.conditions.push(condition.clone());
#         Ok(())
#     }
#     fn set_search(&mut self, text: Option<&str>) -> Result<()> {
#         self.query.search = text.map(str::to_string);
#         Ok(())
#     }
#     fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
#         if let Some((column, _)) = order {
#             self.check_column(column)?;
#         }
#         self.query.order = order.map(|(column, dir)| (column.to_string(), dir));
#         Ok(())
#     }
# }
# fn row(id: &str, name: &str, category: &str) -> (String, Rec) {
#     let cells = [("id", id), ("name", name), ("category", category)];
#     let record = cells.into_iter().map(|(k, v)| (k.to_string(), CborValue::from(v))).collect();
#     (id.to_string(), record)
# }
# fn products() -> Vista {
#     let metadata = VistaMetadata::new()
#         .with_column(Column::new("id", "string").with_flag(ColumnFlag::Orderable))
#         .with_column(Column::new("name", "string").with_flag(ColumnFlag::Searchable))
#         .with_column(Column::new("category", "string"))
#         .with_id_column("id");
#     let rows = vec![row("p1", "Tea", "drink"), row("p2", "Cake", "food"), row("p3", "Coffee", "drink")];
#     Vista::new("product", VecShell::new(Arc::new(rows), metadata))
# }
use vantage_vista::contract;

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
contract::check_advertised(&products()).await;
contract::check_clone(&products(), "category", CborValue::from("drink")).await;
# Ok(())
# }
```

A third check, `check_advertised_writes`, probes the write capabilities. Run it only on a disposable
fixture, since a successful probe stores a row.

Leaving a flag off costs nothing: `Vista` refuses the call, and the caller evaluates the clause
itself or hides the control. So start narrow and add a flag the day its method works. A shell that
ships `Subscribe` the week LIVE queries land is healthier than one that advertised it on day one.
Postgres is the cautionary case for `Subscribe`: `LISTEN` succeeds on a channel no trigger feeds, so
its capability is opt-in (`PostgresVistaFactory::with_notify`), not inferred from the table being
writable. A silent stream can't be told apart from an idle table.

### Tests

Gate the Vista tests on `feature = "vista"`. The contract runs against the real backend, as the
`TableSource` tests in earlier steps do. Around it:

- **Unit tests** that need no backend: the CBOR bridge round-trips every native variant (lossy ones
  assert their documented form), and the YAML spec parses, with an unknown field in the driver block
  failing.
- **Typed and YAML construction**: `from_table` and `from_yaml` list, get and count the seeded rows,
  and the spec name overrides the table name.
- **Push-down**: a condition narrows both `count` and `list_values`, and a second one stacks.
- **Traversal**: `get_ref` from a row and with `None`, for every relation. A test file that never
  traverses won't notice a shell that forgot to forward it.
- **Ids**: `get_value` finds each row by the id `list_values` returned for it.

### Step 8 checklist

At this point your backend has:

1. **A `vista` feature** gating the bridge, and `vantage-vista`'s `contract` feature in
   `[dev-dependencies]`.

2. **`<Driver>::vista_factory()`**, returning a `<Driver>VistaFactory` with an inherent
   `from_table<E>` and an `impl VistaFactory` whose `build_from_spec` routes through the same path.

3. **`<Driver>TableShell`** implementing `TableShell`:
   - `Clone` carries the narrowing and shares the connection;
   - `metadata` from `table.vista_metadata(..)`;
   - `list`, `get`, `count`, the leading window, and `add_condition` with equality, refusing columns
     through `condition_column_error`;
   - `get_ref` through `Table::traverse_erased`, from a row and with `None`;
   - `preview` and `driver_name`;
   - the opt-in methods the backend can honour, and `capabilities()` advertising exactly those.

4. **YAML extras** in `spec.rs`, each with `deny_unknown_fields`, and the id column resolved in the
   fixed order.

5. **Tests**: `check_advertised` and `check_clone` against a real fixture, `check_advertised_writes`
   where the fixture is disposable, and the construction, push-down, traversal and id tests.

The same generic CLI, admin UI or script that drives CSV and MongoDB now drives your backend, with
no entity import and no backend-specific line on the consumer side.
