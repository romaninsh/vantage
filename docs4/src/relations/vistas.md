# Vista, YAML and Rhai

Everything so far assumed you could write `Table<SqliteDB, Order>` in your source code. Generic
consumers can't. A UI grid, an admin panel, a scripting surface, a config-driven tool — none of
them know your model types at compile time. What they hold is a `Vista`: the schema-bearing
runtime handle introduced in [the Vista chapter](../intro/step4-vista.md).

The question this page answers: what happens to relations when the type is erased? The answer is
that they survive in three forms:

1. **Metadata** — the relation names, targets, kinds and join keys are introspectable, so a
   consumer can discover them at runtime.
2. **Traversal** — `get_ref` still works. Hand it a loaded row, get back a new `Vista` narrowed
   to the related rows.
3. **Capabilities** — an honest contract about whether this backend serves traversal at all, so a
   consumer knows what to offer before trying.

### The factory

A `Vista` is built by a **vista factory**: each driver's factory implements the
[`VistaFactory`](vantage_vista::VistaFactory) trait, covered in
[Vista integration](../new-persistence/step8-vista-integration.md). The factory's defining
ability is working **by name**: where typed code passes `User::table(db)`, a generic consumer
asks for `"users"` and gets a Vista back. How the name resolves is the factory's business —
typically it loads table specs from files — and that indirection is what lets config-driven
tools, scripts, and by-name traversal address models they have no Rust types for.

Behind a name, a vista is built one of two ways:

1. **From a typed table** — `from_table` wraps the table your model already builds. One handle
   type comes out, whichever backend went in:

   ```rust
   # use vantage_csv::{AnyCsvType, Csv};
   # use vantage_sql::prelude::*;
   # use vantage_types::prelude::*;
   #
   # #[entity(SqliteType)]
   # #[derive(Debug, Clone, Default)]
   # pub struct Product {
   #     pub name: String,
   # }
   #
   # impl Product {
   #     pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Product> {
   #         Table::new("product", db)
   #             .with_id_column("id")
   #             .with_column_of::<String>("name")
   #     }
   # }
   #
   # #[entity(CsvType)]
   # #[derive(Debug, Clone, Default)]
   # pub struct Order {
   #     pub client_id: String,
   # }
   #
   # impl Order {
   #     pub fn csv_table(csv: Csv) -> Table<Csv, Order> {
   #         Table::new("order", csv).with_column_of::<String>("client_id")
   #     }
   # }
   #
   # #[tokio::main(flavor = "current_thread")]
   # async fn main() -> vantage_core::Result<()> {
   # let sqlite = SqliteDB::connect("sqlite::memory:").await.context("connect")?;
   # let csv = Csv::new("data");
   let vistas = vec![
       sqlite.vista_factory().from_table(Product::sqlite_table(sqlite.clone()))?,
       csv.vista_factory().from_table(Order::csv_table(csv.clone()))?,
   ];
   # assert_eq!(vistas[0].name(), "product");
   # Ok(())
   # }
   ```

2. **From a YAML spec** — no Rust model at all: columns, relations, and computed fields declared
   as data ([Config-Driven Vistas](../config-driven-vistas.md)). This path is how the
   declarations in the last section of this page become live, traversable relations. A spec's
   *source* doesn't have to be a physical table, either — a `rhai:` block builds it from a query
   (a query-sourced vista), and a `base:` block derives it from another spec.

Whichever route constructs it, the factory folds the table's relations into `VistaMetadata` as
[`Reference`](vantage_vista::Reference) entries — name, kind, target, join keys. The erased
handle carries enough to introspect the relations and to traverse them. Nothing about the
relation is lost in the erasure; only the compile-time names are.

### Metadata: introspecting relations

A `Reference` is one model for every relation shape. It is built with the constructor of its
kind, so the kind and its fields always agree:

- `Reference::has_one(name, target, key)` — one target row, found by a key on the parent row.
- `Reference::has_many(name, target, key)` — the target rows that carry the parent's key.
- `Reference::contains_one(name, host_column)` — one record embedded as an object in a column.
- `Reference::contains_many(name, host_column)` — records embedded as an array in a column.

A join's key is a [`JoinKey`](vantage_vista::JoinKey): read the parent row's `from` column and
constrain the target's `to` column to that value (`JoinKey::to_target_id(from)` constrains the
target's id column). A join may carry several keys, and every one of them must match.

`Vista` exposes the entries directly: `references()` is the ordered map of name to `Reference`,
and `reference(name)` looks one up.

```rust
# use vantage_vista::mocks::MockShell;
# use vantage_vista::{Column, JoinKey, Reference, ReferenceKind, Vista, VistaMetadata};
#
let metadata = VistaMetadata::new()
    .with_column(Column::new("id", "String").with_flag("id"))
    .with_column(Column::new("country_id", "String"))
    .with_id_column("id")
    .with_reference(Reference::has_many("orders", "order", JoinKey::new("id", "client_id")))
    .with_reference(Reference::has_one("country", "country", JoinKey::to_target_id("country_id")));
let clients = Vista::new("client", MockShell::new().with_metadata(metadata));

for (name, reference) in clients.references() {
    println!("{name}: {} -> {}", reference.kind.as_str(), reference.target);
}
assert_eq!(clients.reference("orders").unwrap().kind, ReferenceKind::HasMany);
```

This is what lets a UI render relation tabs or context-menu entries without knowing the model.
The loop prints "orders: has_many -> order" and "country: has_one -> country" — enough to label a
tab and decide whether the target is a single row or a set — and it learned that from the
handle, not from your source code. `ReferenceKind::is_contained` separates the embedded kinds
from the joins.

### Traversal: get_ref on a loaded row

The erased twin of the typed `get_ref_from_row` is
[`Vista::get_ref`](vantage_vista::Vista::get_ref), which takes a relation name and an
`Option<&Record<CborValue>>`. Hand it a loaded row — `Record<CborValue>` is the erased record type — and get back a new `Vista`
narrowed to the related rows. The relation is named by string because that's all a generic
consumer has; the reference's keys supply the columns. Each key becomes one equality condition
on the target, applied through `Vista::add_condition`, so the target's `query()` records the join:

```rust
# use ciborium::Value as CborValue;
# use vantage_dataset::ReadableValueSet;
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::{Column, JoinKey, Reference, Vista, VistaMetadata};
#
# fn row(pairs: &[(&str, &str)]) -> Record<CborValue> {
#     pairs.iter().map(|(k, v)| (k.to_string(), CborValue::Text(v.to_string()))).collect()
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let orders = MockShell::new()
#     .with_metadata(
#         VistaMetadata::new()
#             .with_column(Column::new("id", "String").with_flag("id"))
#             .with_column(Column::new("client_id", "String"))
#             .with_id_column("id"),
#     )
#     .with_record("o1", row(&[("id", "o1"), ("client_id", "marty")]))
#     .with_record("o2", row(&[("id", "o2"), ("client_id", "marty")]))
#     .with_record("o3", row(&[("id", "o3"), ("client_id", "doc")]));
# let shell = MockShell::new()
#     .with_metadata(
#         VistaMetadata::new()
#             .with_column(Column::new("id", "String").with_flag("id"))
#             .with_id_column("id")
#             .with_reference(Reference::has_many("orders", "order", JoinKey::new("id", "client_id"))),
#     )
#     .with_record("marty", row(&[("id", "marty")]))
#     .with_ref_target("orders", orders);
# let clients = Vista::new("client", shell);
let marty = clients.get_value("marty").await?.expect("client exists");

let orders = clients.get_ref("orders", Some(&marty))?;
assert_eq!(orders.list_values().await?.len(), 2);

let all_orders = clients.get_ref("orders", None)?;
assert_eq!(all_orders.list_values().await?.len(), 3);
# Ok(())
# }
```

Passing `None` gives the bare, unconditioned target. This is the insert destination for nested
creates: when a UI wants to add a related row, it needs the target table without any narrowing
applied. A contained relation works the same way: from a row, `get_ref` returns a Vista over
the records embedded in the row's host column; with `None`, the empty contained schema.

Both forms return `Vista`, so traversal chains and the consumer never leaves the erased world. A
grid that drills from clients into orders into line items is calling `get_ref` three times, each
time holding nothing more specific than a `Vista` and a row.

```admonish info title="Record ids that round-tripped through JSON"
At the erased layer, SurrealDB record ids travel as strings (`"table:key"`). The backend coerces
a string-shaped record id back into a typed record id when narrowing, so a row that round-tripped
through JSON or a script still traverses correctly.
```

### The scripting surface

Rhai data scripts (the `vantage-vista` rhai feature) traverse with `ref(relation)` on a table
handle, which follows the relation from every row of a set. A one-row set goes through `get_ref`,
so it follows the same metadata and backend traversal described here. Several rows become an `in`
condition on the bare target's join column (which needs a relation joined on one key), a
`limit(n)` or `offset(n)` before the `ref` picks the rows it follows, and an empty set gives an empty target
without a query. See [The Table Handle](../rhai/tables.md#following-relations).

A relation's foreign key may be a [computed column](../rhai/computed.md) (a `lazy:` column). A
has-one through one works, because the parent row carries the computed value. A has-many whose key
is a computed column of the child is refused by `get_ref` with an error naming the relation and
column, since the backend can't filter the child by a value it doesn't store.

### Capabilities: the honest contract

Not every backend serves every traversal form — the [traversal chapter](./traversal.md) already
showed that set-level traversal needs subqueries. At the typed layer, you knew that when you wrote
the code. At the erased layer, the consumer has to ask. That's what
[`VistaCapabilities`](vantage_vista::VistaCapabilities) is for: a set of
[`Capability`](vantage_vista::Capability) values, each naming one method family the shell answers
without refusing. The ones that matter for relations:

- `Capability::Traverse` — the shell answers `get_ref`. A shell without it refuses every
  traversal with an `Unsupported` error naming the capability.
- `Capability::FilterOps` — the shell takes operator conditions, `in` among them. A script's
  multi-row `ref` narrows the target with an `in` condition, so it needs this on the target.
- `Capability::ConfineWrites` — writes through a narrowed Vista stay inside the narrowed set. An
  insert into the result of `get_ref` is such a write.

Consumers branch on these before offering the corresponding affordance: a grid checks
`vista.capabilities().has(Capability::Traverse)` before showing relation tabs, and a nested-create
form checks the target's `Insert` and `ConfineWrites`. The same set appears in the Rhai
`capabilities()` map under each capability's snake_case name (`traverse`, `filter_ops`), so
scripts and config-driven tools can branch the same way.

Two relation features need no flag of their own. A reference with a Rhai build script carries it
in `Reference::build_script`, and the shell runs it in place of the key match (SurrealDB, MongoDB
and MySQL accept one). A dotted column either lowers when the vista is built or fails the build,
so a vista that exists already serves every dotted column it declares.

### Across datasources: the Catalog

Everything above stays inside one datasource — a `Vista`'s references forward to the wrapped
table, and a single Vista deliberately knows nothing about *other* datasources. Traversal across
datasources lives one layer up, in a [`Catalog`](vantage_vista::Catalog): a map from model name to
a loader that builds a fresh, unconditioned Vista, whichever backend backs it.

You register models by name with `Catalog::with`, then narrow a target the catalog builds with
`Reference::narrow`. The reference is the same `Reference` the metadata holds; here it names a
model in another backend:

```rust
# use ciborium::Value as CborValue;
# use vantage_dataset::ReadableValueSet;
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::{Catalog, Column, JoinKey, Reference, Vista, VistaMetadata, VistaResolver};
#
# fn row(pairs: &[(&str, &str)]) -> Record<CborValue> {
#     pairs.iter().map(|(k, v)| (k.to_string(), CborValue::Text(v.to_string()))).collect()
# }
#
# fn meta(columns: &[&str]) -> VistaMetadata {
#     columns
#         .iter()
#         .fold(VistaMetadata::new(), |m, c| m.with_column(Column::new(*c, "String")))
#         .with_id_column("id")
# }
#
# // Stands in for a client table in one backend.
# fn client_vista() -> Vista {
#     let shell = MockShell::new()
#         .with_metadata(meta(&["id", "bakery_id"]))
#         .with_record("marty", row(&[("id", "marty"), ("bakery_id", "hill-valley")]));
#     Vista::new("client", shell)
# }
#
# // Stands in for a bakery API in another.
# fn bakery_vista() -> Vista {
#     let shell = MockShell::new()
#         .with_metadata(meta(&["id", "name"]))
#         .with_record("hill-valley", row(&[("id", "hill-valley"), ("name", "Hill Valley")]))
#         .with_record("twin-pines", row(&[("id", "twin-pines"), ("name", "Twin Pines")]));
#     Vista::new("bakery", shell)
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let catalog = Catalog::new()
    .with("client", |_: &str| Ok(client_vista()))
    .with("bakery", |_: &str| Ok(bakery_vista()));

let bakery = Reference::has_one("bakery", "bakery", JoinKey::new("bakery_id", "id"));

let marty = catalog.vista("client")?.get_value("marty").await?.expect("client exists");
let hers = bakery.narrow(catalog.vista(&bakery.target)?, &marty)?;
assert_eq!(hers.list_values().await?.len(), 1);
# Ok(())
# }
```

`narrow` applies one equality condition per key, through `Vista::add_condition`. How each
condition is honoured — SQL `WHERE`, in-memory filter, REST path or query parameter — is the
target driver's concern at fetch time.

A name the catalog doesn't hold is a not-found error, and a loader that fails keeps its own
error. Because loaders build a fresh Vista on every call, the catalog composes with hot-reloading
inventories. The catalog is a [`VistaResolver`](vantage_vista::VistaResolver), so it goes wherever
a resolver is expected — the Dio layer's augmentation resolves its detail sources through one,
which the [next page](./dio.md) picks up.

#### Why not register cross-datasource relations on the catalog?

Because the relation already has a home: the `Reference`. A second relation type on the catalog
would be one more spelling of "read this column, constrain that one", with its own constructor and
its own traversal call. Keeping one model means a reference read out of a vista's metadata and a
reference written by hand for a cross-backend join narrow the same way. The catalog stays what it
is good at: building a model by name.

### Declaring relations in YAML and Rhai

Config-driven vistas (full chapter: [Config-driven vistas](../config-driven-vistas.md)) declare
relations in the table spec — either as column-level sugar or a top-level map. The factory reads
the spec, registers the relations, and the resulting `Vista` traverses them exactly like one built
`from_table`:

```yaml
name: product
columns:
  id:
    type: string
    flags: [id]
  batch:
    type: string
    references: batch        # sugar: has_one named `batch`, FK = this column
  batch.name:                # implicit reference through that relation
    type: string
references:
  tags:
    table: tag
    kind: has_many
    foreign_key: product_id
```

The column-level `references: batch` sugar names the relation after the *column* and uses the
column as the foreign key, matched to the target's id. That is the common case: a column named
after the table it points at. A `references:` entry with the same name takes precedence over the
sugar. The full form (`table`, `kind`, `foreign_key`) names the relation independently of any
column — you need that when two relations point at the same table.

A join that needs more than one column pair lists them under `keys:`. Each pair reads the parent
row's `from` column and constrains the target's `to` column, and the child must match every pair:

```yaml
references:
  deployments:
    table: deployment
    kind: has_many
    keys:
      - { from: product_id, to: product }
      - { from: version_id, to: version }
```

When `keys:` is given it describes the whole join; `foreign_key:` applies only when it is absent.

When a relation needs more than key matches — extra conditions, ordering, a search — give the
reference a `rhai:` build script in its driver block (SurrealDB, MongoDB and MySQL accept one):

<!-- tested: rhai_guide::surfaces::ref_build (the script) -->
```yaml
references:
  due_orders:
    table: order
    kind: has_many
    foreign_key: client
    surreal:
      rhai: |
        table("order").where("client", row.id).where("status", "due")
```

The script runs lazily when the relation is traversed, with the parent record in scope as `row`,
and must end on a table handle: start it with `table("<name>")` and chain the
[narrowing verbs](../rhai/tables.md#narrowing) (see
[Reference build scripts](../rhai/surfaces.md#reference-build-scripts)). Without `rhai:`, the
relation falls back to the plain key match. The script lands in the reference's `build_script`,
which the shell evaluates instead of the keys.

Imported dotted columns — like `batch.name` above, the YAML form of
[implicit references](./implicit-references.md) — go through the same traversal import as the
Rust API, with the same construction-time validation: a bad dotted column fails the spec load,
not the first fetch. They arrive in metadata flagged `calculated`. That flag means read-only for
consumers: the value comes from a traversal, not from a column you can write. This is how a UI
knows not to offer editing on them. [Computed columns](../rhai/computed.md) (`lazy:`) carry the
same flag for the same reason.

The key difference: at the typed layer *you* know which traversal forms are safe — you wrote the
code against a backend you chose. At the erased layer, the *capabilities* say so. Same relations,
discovered instead of known.

### Conclusion

At this point you should be able to:

1. **Enumerate a Vista's relations** — `references()` for every `Reference`, `reference(name)` for
   one, its `kind` for the cardinality and `keys` for the join.
2. **Build a reference by hand** — `Reference::has_one`, `has_many`, `contains_one`,
   `contains_many`, with `JoinKey::new` or `JoinKey::to_target_id`.
3. **Traverse from a loaded erased row** — `get_ref(relation, Some(&row))` returns a narrowed
   `Vista`; `get_ref(relation, None)` returns the bare target for nested creates.
4. **Check the capability first** — `Capability::Traverse` before offering traversal,
   `FilterOps` and `ConfineWrites` on the target for multi-row script traversal and nested
   inserts.
5. **Traverse across datasources** — register models with `Catalog::with`, then
   `reference.narrow(catalog.vista(&reference.target)?, &row)`.
6. **Declare relations in YAML and Rhai** — column-level `references:` sugar, the full top-level
   form, `keys:` for multi-column joins, implicit dotted columns through a relation, and `rhai:`
   build scripts for traversals that need more than a key match — all consumed by the same vista
   factory that wraps typed tables.
7. **Explain why `calculated` columns are read-only** in generic UIs — their values come from
   traversal, not from a writable column.

Next: [what happens above the Vista](./dio.md) — combining erased handles across backends.
