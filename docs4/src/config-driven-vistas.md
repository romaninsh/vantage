# Config-Driven Vistas: YAML & Rhai

The [Vista chapter](./intro/step4-vista.md) built Vistas by wrapping typed tables — Rust code,
compiled in. This page covers the other construction path: **declare the Vista in YAML, load it at
runtime, and seal it behind the same honest handle**. Change the YAML, rebuild the Vista, and the
consumer sees the new shape — no recompiling.

This is the path configuration-driven tooling takes: admin panels reading a folder of model files,
AI agents writing schema on a user's behalf, and any application (Vantage UI among them) whose data
layer is user-editable data rather than user-compiled code. For everything YAML can't express —
vendor expressions, derived queries, scripted traversal — there's an optional **Rhai** layer that
compiles to native queries.

<!-- toc -->

---

## One page of YAML, one working Vista

Every driver's factory implements [`VistaFactory`](vantage_vista::VistaFactory), whose `from_yaml`
parses a spec and lowers it. `from_yaml` is a trait method, so the trait has to be in scope; it is
imported by name from `vantage_vista`, not through a prelude. The block reads a populated `product`
table, so it is compiled but not run:

```rust,no_run
# use vantage_dataset::ReadableValueSet;
# use vantage_sql::prelude::*;
use vantage_vista::VistaFactory;

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db").await.context("connect")?;
let yaml = r#"
name: product_view
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
  price:
    type: int
sqlite:
  table: product
"#;

let vista = db.vista_factory().from_yaml(yaml)?;

assert_eq!(vista.name(), "product_view");
assert_eq!(vista.id_column(), Some("id"));
let rows = vista.list_values().await?;   // reads the `product` table
# let _ = rows;
# Ok(())
# }
```

The factory builds a typed `Table` under the hood — each `type:` becomes a real `Column<T>` — and
wraps it exactly as `from_table` would. From here on, nothing downstream can tell how the Vista was
made: same schema introspection, same conditions, same capability contract.

Two implicit rules worth knowing:

- **The Vista's name is the spec's `name`**, not the storage name. `product_view` is what a UI tab
  or a catalog key sees; `sqlite: { table: product }` is where the rows live. Omit the block and
  the spec name doubles as the table name.
- **The id column resolves in a fixed order**: explicit `id_column:` at the top level wins, then
  the first column flagged `id`, then a driver default (`"id"` for SQL, `"_id"` for MongoDB). The
  explicit key exists so a YAML author can correct a wrong flag without touching anything else.

---

## Anatomy of a spec

A [`VistaSpec`](vantage_vista::VistaSpec) has a uniform core that every driver understands, plus
one driver-named block at each level:

| Key          | What it is                                                          |
| ------------ | ------------------------------------------------------------------- |
| `name`       | The vista's public name (catalog key, UI label)                     |
| `datasource` | Optional datasource key, for inventories that manage several        |
| `id_column`  | Explicit id override (see resolution order above)                   |
| `columns`    | Ordered map of column name → `{ type, flags, references, lazy, <driver> }`|
| `references` | Named relations to other vistas (see below)                         |
| `contained`  | Embedded-in-row relations (see below)                               |
| `<driver>`   | The driver's table-level block: `sqlite:`, `surreal:`, `mongo:`, `csv:` |

### Column types

`type:` names lower to real typed columns; unknown names are a **parse error**, not a fallback:

| YAML `type`                      | Rust column           |
| -------------------------------- | --------------------- |
| `int`, `integer`, `i64`, `i32`   | `Column<i64>`         |
| `float`, `double`, `f64`, `f32`  | `Column<f64>`         |
| `bool`, `boolean`                | `Column<bool>`        |
| `string`, `text`, `str` (default)| `Column<String>`      |
| `decimal`, `numeric`             | `Column<Decimal>`     |
| `date` / `time` / `datetime`     | chrono naive types    |
| `timestamp`                      | `DateTime<Utc>`       |

`flags:` is the same open vocabulary the Vista chapter introduced — `id`, `title`, `searchable`,
`orderable`, `hidden`, `mandatory` — and drives the same consumer behaviour (title columns label
rows, `searchable` feeds quicksearch, `hidden` drops out of default views).

### Typos fail at parse time

Every driver block sets `deny_unknown_fields`, so `sqlite: { tabel: product }` is rejected when the
YAML is parsed — with the offending key in the message — instead of silently falling back to a
default and failing later at query time. Driver-block validation follows the same rule: an empty
`nested_path`, a `a..b` path, mutually-exclusive options — all surface as parse errors naming the
column.

### Column mapping per driver

When the spec column name differs from the storage name, the column's driver block maps it:

| Driver  | Column block                     | Meaning                                                  |
| ------- | -------------------------------- | -------------------------------------------------------- |
| SQLite  | `sqlite: { column: unit_price }` | SELECTed under the SQL name, aliased back to the spec name |
| Surreal | `surreal: { field: unitPrice }`  | Read/written under the Surreal field name                 |
| MongoDB | `mongo: { field: unitPrice }`    | Single-level BSON rename                                  |
| MongoDB | `mongo: { nested_path: price.amount }` | Dotted path into a nested document — projected on read, sub-documents rebuilt on write, dotted form pushed down on filter |
| CSV     | `csv: { source: "Unit Price" }`  | CSV header to read from                                   |
| GraphQL | `graphql: { field: unit_price }` | Selected as a GraphQL alias, filtered and sorted under the server name ([GraphQL blocks](./http-apis/graphql-yaml.md)) |

MongoDB adds the column types `object_id`, `datetime` (aliases `date`, `timestamp`; a BSON
DateTime) and `decimal` (Decimal128). Writes keep these types, and text that doesn't parse as the
column's type is rejected. A column named with a dotted path through a reference (`client.name`)
joins via `$lookup`, which compares types strictly: a string foreign key does not join an
`ObjectId` `_id`.

A MongoDB table block can replace the collection read with an aggregation pipeline, or script it:

| Table block          | Meaning                                                              |
| -------------------- | -------------------------------------------------------------------- |
| `mongo: { pipeline: [...] }` | Read-only view over an aggregation pipeline (`MongoView`)    |
| `mongo: { rhai: "..." }`     | Builds the pipeline in Rhai from `args` (`rhai` feature)     |
| `mongo: { modify: "..." }`   | Rhai run last against the built vista as `self`              |
| `mongo: { view: true }`      | The collection is a server-side view: read-only, not watched, not imported |

Change streams (`Capability::Subscribe`) and atomic imports (`Capability::Import`) need a replica
set; see [Live data](./live-data.md). Streams run on a dedicated client sized by
`MongoDB::with_stream_pool_size` (default 100).

CSV is also the one driver whose *table* block is mandatory — `csv: { path: data/products.csv }` —
because without a path there is no file.

---

## References

A relation to another vista, in the same uniform vocabulary the typed `with_one`/`with_many` uses:

```yaml
name: category
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: category_id
```

Each entry lowers to one [`Reference`](vantage_vista::Reference) in the vista's metadata. Three
shapes are available:

- **Full form** (above): `table`, `kind` (`has_one` / `has_many`), `foreign_key`. A `has_one`
  matches its `foreign_key` on this row to the target's id; a `has_many` matches this row's id to
  the target's `foreign_key`. Without `foreign_key:`, the entry's name is the key.
- **Column sugar** — when the foreign key *is* the column, declare it inline. The relation takes
  the column's name, and a `references:` entry of the same name takes precedence:

  ```yaml
  columns:
    category_id:
      type: string
      references: category      # has_one named `category_id`, foreign_key = the column itself
  ```

- **Multi-key joins** — `keys:` lists `{ from, to }` pairs when the child is narrowed by more than
  one parent field (a deployment matched on both `product_id` and `version_id`). Each pair reads
  the parent row's `from` column and constrains the target's `to` column, and traversal applies
  every pair. When `keys:` is present it is the whole join, and `foreign_key:` is not read:

  ```yaml
  references:
    deployments:
      table: deployment
      kind: has_many
      keys:
        - { from: product_id, to: product }
        - { from: version_id, to: version }
  ```

On MongoDB a reference key must be stored under its own field name. The vista fails to build when a
reference's foreign key, or the id column it matches on the other side, is renamed with `field:` or
moved with `nested_path:`. A reference with a `mongo: { rhai: }` script (below) is exempt, as its
traversal runs the script. Joined columns (`client.city`) can still read nested fields of the target.

### The spec resolver

A reference names its target *by spec name* — so the factory needs a way to find the target's
current spec at traversal time. That's the **spec resolver**, attached once:

```rust
# use std::collections::HashMap;
# use std::sync::Arc;
#
# use ciborium::Value as CborValue;
# use vantage_sql::prelude::*;
# use vantage_sql::sqlite::vista::SqliteVistaSpec;
# use vantage_types::Record;
# use vantage_vista::{ColumnSpec, SpecResolver, VistaFactory};
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite::memory:").await.context("connect")?;
# let category_yaml = r#"
# name: category
# columns:
#   id: { type: string, flags: [id] }
#   name: { type: string, flags: [title] }
# references:
#   products:
#     table: product
#     kind: has_many
#     foreign_key: category_id
# "#;
# let mut product = SqliteVistaSpec::new("product");
# product.columns.insert("id".to_string(), ColumnSpec::new().with_type("string").with_flag("id"));
# product.columns.insert("category_id".to_string(), ColumnSpec::new().with_type("string"));
# let specs: HashMap<String, SqliteVistaSpec> = HashMap::from([("product".to_string(), product)]);
let resolver: Arc<dyn SpecResolver<SqliteVistaSpec>> =
    Arc::new(move |name: &str| specs.get(name).cloned());
let factory = db.vista_factory().with_resolver(resolver);
let category = factory.from_yaml(category_yaml)?;

// Traversal rebuilds `product` from its live spec:
let row: Record<CborValue> = [("id".to_string(), CborValue::Text("pastry".into()))].into_iter().collect();
let products = category.get_ref("products", Some(&row))?;
# assert!(products.is_narrowed());
# Ok(())
# }
```

[`SpecResolver<S>`](vantage_vista::SpecResolver) is the same trait for every driver, with the
driver's spec type as `S`, and a factory holds it as `Arc<dyn SpecResolver<S>>`. Any
`Fn(&str) -> Option<S>` closure implements it (annotate its argument as `&str`, as above, or the
closure won't take every lifetime), and a type of your own can implement `resolve` directly — back
it with an in-memory map, a folder of `.yaml` files, or a hot-reloading inventory. Because targets
are re-resolved on every traversal, editing a spec changes what the *next* traversal builds.
Without a resolver, a traversal falls back to a column-less target and the next query fails
loudly.

---

## The Rhai layer

YAML declares structure. For anything with an expression in it, specs escalate to **Rhai** — a
small, embeddable scripting language whose Vantage vocabulary compiles to native queries. The
expression primitives are shared across backends where the concept overlaps (`count`, `avg`,
`coalesce`, `case_when`, `date_format` …) — see [SQL Primitives](./sql/primitives.md) and
[SurrealDB Primitives](./surrealdb/primitives.md) for the full vocabularies. A script calls them
through its backend's namespace (`sql::count(..)`, `surreal::count(..)`, `mongo::oid(..)`), so they
never shadow the data vocabulary's `table(name)`. Rhai appears in a spec
in five places, each with a distinct job. The first two build native queries; the next two use the
data vocabulary taught in [Scripting with Rhai](./rhai.md); the last computes a column from one
row.

### 1. Query-sourced vistas — `rhai:`

Replace the physical table with a script-built SELECT:

```yaml
name: expensive_products
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
sqlite:
  rhai: |
    sql::select().from("product").field("id").field("name").where(sql::expr("price > 150"))
```

The script runs once at build time and its SELECT becomes the vista's source. A query-sourced
vista is **read-only** — the factory leaves `Insert`, `Update`, `Delete` and `ConfineWrites` out
of its capabilities, because there's no single table a write could honestly land in. Capability honesty is preserved
automatically; consumers find out by checking, same as ever.

### 2. Derived vistas — `base:` + `inherit` + transform

Derive one vista from another, inheriting schema and transforming the query. The base's `select()`
is seeded into the script's scope as `base`:

```yaml
name: category_totals
id_column: category_id
columns:
  total_price: { type: int }
sqlite:
  base: product
  inherit:
    columns: [category_id]
  rhai: |
    base.clear_fields().field("category_id")
        .expression(sql::expr("SUM(price) AS total_price"))
        .group_by(sql::expr("category_id"))
```

This is the aggregate pattern: group the base by a key, declare the aggregate outputs as the
derived vista's own `columns:`, and re-key with `id_column`. `base:` resolves eagerly through the
same spec resolver as references. Derived vistas are query-sourced, hence read-only.

### 3. Post-build tweaks — `modify:` (SurrealDB, MongoDB, MySQL)

A script applied to the *finished* vista, exposed as `self`, using the
[table handle's verbs](./rhai/tables.md) plus vendor expressions YAML keys can't state (see
[`modify:` scripts](./rhai/surfaces.md#modify-scripts)):

```yaml
name: active_products
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
surreal:
  table: product
  modify: |
    self.with_condition(surreal::ident("is_deleted") == false)
        .sort("name", "asc")
```

Unlike a query source, `modify:` narrows a real table — so the vista **stays writable**. It runs
last, composing with `table:`, `rhai:`, or `base:`. Writability follows the underlying source: a
`modify:` on a `rhai:` or `base:` vista stays read-only, while on a plain table it stays writable.
MySQL takes the same script in a `mysql:` block (`mysql: { modify: "..." }`), with the
`with_condition(expr)` verb, which hands its expression to the shell as a native condition (from
Rust: `Vista::add_native_condition`).

### 4. Scripted reference traversal — reference-level `rhai:` (SurrealDB, MongoDB, MySQL)

When a relation can't be expressed as a foreign-key equality — a graph edge, a computed join — the
reference carries its own build script, evaluated lazily at traversal time with the parent `row` in
scope:

```yaml
references:
  products:
    table: product
    kind: has_many
    foreign_key: category
    surreal:
      rhai: |
        table("product").where("category", row.id)
```

The script ends on a table handle, which becomes the narrowed target vista; `foreign_key` remains as metadata for consumers that
introspect the relation. See [Reference build scripts](./rhai/surfaces.md#reference-build-scripts).

MongoDB takes the same script in the reference's `mongo:` block (`rhai` feature):

```yaml
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client_id
    mongo:
      rhai: |
        table("order").where("client_id", row._id)
```

MySQL takes it in the reference's `mysql:` block (`mysql: { rhai: "..." }`, `rhai` feature).

### 5. Computed columns — column-level `lazy:` (every driver)

A column whose value no backend stores, computed by the Vista from the row it read:

<!-- tested: rhai_guide::computed::declare_in_yaml (excerpt) -->
```yaml
columns:
  net: { type: int }
  vat: { type: int, lazy: "row.net / 5" }
```

The script sees the row as `row`, and its last expression is the value. Columns compute in
declaration order, writes drop them, and filtering, ordering or aggregating on one is an error
naming the column. A has-one reference may use one as its foreign key; a has-many may not. Full
details in [Computed Columns](./rhai/computed.md).

```admonish note title="YAML primary, Rhai targeted"
The division of labour is deliberate: YAML stays the canonical, declarative format that every
backend understands; Rhai is the serializable escape hatch you reach for only when a source,
transform, or traversal needs an expression. A spec with no Rhai in it works on a build without the
`rhai` feature; one that uses it fails loudly there instead of degrading.
```

---

## Contained relations

Embedded objects and arrays — an order's `lines`, a JSON column — declare as a `contained:`
section and surface as editable sub-vistas:

```yaml
name: order
columns:
  id: { type: string, flags: [id] }
  lines: { type: string }          # the host column — declare it so it's selected
contained:
  lines:
    host_column: lines
    kind: contains_many
    id_column: line_id             # omit for positional ids
    columns:
      product: { type: string }
      quantity: { type: int }
```

Reads project the embedded collection into records; writes patch it back into the host column. The
mechanics — and the sharp edges — are covered in
[Contained Relations](./new-persistence/step9-contained-relations.md).

---

## Cross-persistence: the Catalog

A single Vista is strictly single-backend. When a system spans several — categories in Postgres,
products behind a REST API — a [`Catalog`](vantage_vista::Catalog) sits one layer up: it holds a
loader per model name, and a relation whose target lives in a *different* persistence is a
[`Reference`](vantage_vista::Reference) narrowed over the target the catalog builds. Here two
in-memory SQLite databases stand in for the two backends:

```rust
# use ciborium::Value as CborValue;
# use vantage_sql::prelude::*;
# use vantage_types::Record;
# use vantage_vista::{Catalog, JoinKey, Reference, VistaFactory, VistaResolver};
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let pg = SqliteDB::connect("sqlite::memory:").await.context("connect")?;
# let api = SqliteDB::connect("sqlite::memory:").await.context("connect")?;
# let category_yaml = "name: category\ncolumns:\n  id: { type: string, flags: [id] }\n".to_string();
# let product_yaml =
#     "name: product\ncolumns:\n  id: { type: string, flags: [id] }\n  category_id: { type: string }\n"
#         .to_string();
let (pg_factory, api_factory) = (pg.vista_factory(), api.vista_factory());
let catalog = Catalog::new()
    .with("category", move |_: &str| pg_factory.from_yaml(&category_yaml))
    .with("product", move |_: &str| api_factory.from_yaml(&product_yaml));
let relation = Reference::has_many(
    "products",
    "product",
    JoinKey::new("id", "category_id"), // parent `id` constrains the target's `category_id`
);

let row: Record<CborValue> = [("id".to_string(), CborValue::Text("pastry".into()))].into_iter().collect();
let products = relation.narrow(catalog.vista(&relation.target)?, &row)?;  // one backend → the other
# assert!(products.is_narrowed());
# Ok(())
# }
```

Loaders return a *fresh, unconditioned* Vista on every call, so the catalog composes with
hot-reloading inventories the same way spec resolvers do. A name the catalog doesn't hold is a
not-found error; a loader that fails keeps its own error. The catalog is a
[`VistaResolver`](vantage_vista::VistaResolver), so it goes wherever a resolver is expected: it is
also what [Augmentation](./augmentation.md) uses to resolve its detail sources — the `augment:`
block in a table's configuration is this same machinery pointed at row enrichment.

---

## Sealed at runtime: data scripts

Once vistas are config-defined, the last step of the story is *consuming* them from config too.
`vantage-vista`'s `rhai` feature ships `run_script`, a sandboxed evaluator where `table(name)`
resolves through a catalog-style resolver. It's the surface an AI agent or an MCP tool drives: the schema it sees, the capabilities it must
respect, and the rows it reads all come from the same sealed handles this page built. The
vocabulary, its limits and every other place it runs are covered in
[Scripting with Rhai](./rhai.md).

---

## Choosing a path

Both construction paths produce the same `Vista` — the choice is about who edits the definition:

- **Typed `from_table`** when the model is code: business logic, compile-time safety, entity
  structs, `with_expression` closures. This is the path the [introduction guide](./intro/step4-vista.md)
  walks.
- **YAML (+ Rhai)** when the model is data: inventories on disk, user- or agent-edited schema,
  hot-reload, no recompiling. Structure in YAML; expressions in Rhai; capabilities sealed either
  way.

They mix freely — a catalog can hold typed-table loaders next to YAML loaders, and a YAML spec's
reference can resolve to either. If you're implementing this machinery for a new backend, the
driver-side walkthrough is [Adding a New Persistence, Step 8](./new-persistence/step8-vista-integration.md).
