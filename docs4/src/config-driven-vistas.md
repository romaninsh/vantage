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
parses a spec and lowers it:

```rust
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
assert_eq!(vista.get_id_column(), Some("id"));
let rows = vista.list_values().await?;   // reads the `product` table
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
| `columns`    | Ordered map of column name → `{ type, flags, references, <driver> }`|
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

Three shapes are available:

- **Full form** (above): `table`, `kind` (`has_one` / `has_many`), `foreign_key`.
- **Column sugar** — when the foreign key *is* the column, declare it inline:

  ```yaml
  columns:
    category_id:
      type: string
      references: category      # has_one, foreign_key = the column itself
  ```

- **Multi-key joins** — `keys:` lists `{ to, from }` pairs when the child is narrowed by more than
  one parent field (a deployment matched on both `product_id` and `version_id`).

### The spec resolver

A reference names its target *by spec name* — so the factory needs a way to find the target's
current spec at traversal time. That's the **spec resolver**, attached once:

```rust
let resolver: SqliteSpecResolver = Arc::new(move |name| specs.get(name).cloned());
let factory = db.vista_factory().with_resolver(resolver);
let category = factory.from_yaml(category_yaml)?;

// Traversal rebuilds `product` from its live spec:
let row = /* a category row */;
let products = category.get_ref("products", &row)?;
```

The resolver is a plain closure — back it with an in-memory map, a folder of `.yaml` files, or a
hot-reloading inventory. Because targets are re-resolved on every traversal, editing a spec changes
what the *next* traversal builds. Without a resolver, a traversal falls back to a column-less
target and the next query fails loudly.

---

## The Rhai layer

YAML declares structure. For anything with an expression in it, specs escalate to **Rhai** — a
small, embeddable scripting language whose Vantage vocabulary compiles to native queries. The
expression primitives are shared across backends where the concept overlaps (`count`, `avg`,
`coalesce`, `case_when`, `date_format` …) — see [SQL Primitives](./sql/primitives.md) and
[SurrealDB Primitives](./surrealdb/primitives.md) for the full vocabularies. Rhai appears in a spec
in four places, each with a distinct job.

### 1. Query-sourced vistas — `rhai:`

Replace the physical table with a script-built SELECT:

```yaml
name: expensive_products
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
sqlite:
  rhai: |
    select().from("product").field("id").field("name").where(expr("price > 150"))
```

The script runs once at build time and its SELECT becomes the vista's source. A query-sourced
vista is **read-only** — the factory clears `can_insert` / `can_update` / `can_delete`, because
there's no single table a write could honestly land in. Capability honesty is preserved
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
        .expression(expr("SUM(price) AS total_price"))
        .group_by(expr("category_id"))
```

This is the aggregate pattern: group the base by a key, declare the aggregate outputs as the
derived vista's own `columns:`, and re-key with `id_column`. `base:` resolves eagerly through the
same spec resolver as references. Derived vistas are query-sourced, hence read-only.

### 3. Post-build tweaks — `modify:` (SurrealDB)

A script applied to the *finished* vista, exposed as `self`, using the builder verbs plus vendor
expressions YAML keys can't state:

```yaml
name: active_products
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
surreal:
  table: product
  modify: |
    self.with_condition(ident("is_deleted") == false)
        .add_order("name", "asc")
```

Unlike a query source, `modify:` narrows a real table — so the vista **stays writable**. It runs
last, composing with `table:`, `rhai:`, or `base:`.

### 4. Scripted reference traversal — reference-level `rhai:` (SurrealDB)

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
        table("product").add_condition_eq("category", row.id)
```

The script returns the narrowed target vista; `foreign_key` remains as metadata for consumers that
introspect the relation.

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

## Cross-persistence: the VistaCatalog

A single Vista is strictly single-backend. When a system spans several — categories in Postgres,
products behind a REST API — [`VistaCatalog`](vantage_vista_factory) sits one layer up: it holds a
loader per model name and traverses relations whose target lives in a *different* persistence:

```rust
let mut catalog = VistaCatalog::new();
catalog.register("category", Arc::new(move || pg_factory.from_yaml(&category_yaml)));
catalog.register("product",  Arc::new(move || api_factory.from_yaml(&product_yaml)));
catalog.register_relation("category", Relation::single_key(
    "products", "product", ReferenceKind::HasMany,
    "category_id",   // target column to constrain
    "id",            // parent-row field whose value narrows it
));

let category = catalog.build_vista("category")?;
let row = /* a category row */;
let products = catalog.traverse_from("category", "products", &row)?;  // Postgres → REST
```

Loaders return a *fresh, unconditioned* Vista on every call, so the catalog composes with
hot-reloading inventories the same way spec resolvers do. The catalog is also what
[Augmentation](./augmentation.md) uses to resolve its detail sources — the `augment:` block in a
table's configuration is this same machinery pointed at row enrichment.

---

## Sealed at runtime: data scripts

Once vistas are config-defined, the last step of the story is *consuming* them from config too.
`vantage-vista`'s `rhai` feature ships `run_script` — a sandboxed evaluator where `table(name)`
resolves through a catalog-style resolver and a handful of read verbs fetch data:

```rhai
let o = table("orders").add_condition_eq("status", "unpaid").get_some();
if o != () {
    table("orders").get_ref("client", o).get_some()
}
```

The vocabulary is deliberately small: builder verbs (`add_condition_eq`, `add_order`, `get_ref` …)
plus terminals — `list`, `get_some`, `count`, `capabilities`, `columns`, `references`. Every
`list()` is capped (50 rows hard ceiling) — this is an inspection and automation surface, not a
bulk reader. It's the surface an AI agent or an MCP tool drives: the schema it sees, the
capabilities it must respect, and the rows it reads all come from the same sealed handles this
page built.

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
