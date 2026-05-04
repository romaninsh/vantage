# Step 7: Vista Integration

By the end of Step 6 your backend is a fully-featured persistence — typed `Table<T, E>`, conditions,
relationships, the lot. That's exactly what business logic wants. It's also exactly what generic
code can't consume.

A CLI that lists "any table from any backend" can't carry an entity generic. A web admin that draws
forms from a YAML schema doesn't know your `Product` struct exists. A Rhai script that filters rows
by a string field name shouldn't have to compile against your backend's condition type. **Vista** is
the bridge: a schema-bearing handle that wraps a typed `Table` and exposes it through a uniform,
CBOR-typed surface.

This step adds Vista support to your backend. The work is small — one factory, one source, a few
hundred lines — but it's the doorway through which UI, CLI, and config-driven tooling start
seeing your database at all.

### Before you start

Vista is a thin layer over what Steps 1–6 already gave you. Most of the work here is decisions
rather than code. Confirm you have:

- A working `TableSource` impl (Step 4) — Vista's read/write path delegates to it unchanged.
- A native value type with an `Into<CborValue>` story (Step 1). If your value type is already JSON-
  shaped you're done; otherwise you need a dedicated bridge (see `cbor.rs` below).
- An id type that round-trips through `String` — `FromStr` and `Display` impls. If the native id
  doesn't have these yet, add them before going further. The vista boundary stringifies ids
  unconditionally.

### What Vista actually is

`Vista` is a concrete struct in `vantage-vista` (no consumer-facing trait surface). It owns
universal metadata — name, columns, references, capabilities, id column — and a boxed `TableShell`
that does the real work. Your job as a driver author is two-fold:

1. A **factory** that produces a `Vista` from either a typed `Table<YourDB, E>` or a YAML schema.
2. A **source** that implements `TableShell` — the per-driver executor `Vista` delegates to.

Both construction paths converge on the same source-creation code. That's a deliberate constraint:
post-construction Vista usage is fully database-agnostic, so the same UI/CLI/script drives a Mongo
Vista, a SurrealDB Vista, or your CSV one without caring how it got built.

### Why not just hand around `Table<T, E>`?

You can! Anywhere the entity is known at compile time, `Table<T, E>` is the better tool — it's
typed, it composes, and it's what you've spent six steps building. Vista exists for the cases where
the entity *isn't* known at compile time, or where the backend itself is chosen at runtime:

- A CLI driven by `--source surreal --table client list` — no `Client` struct in scope.
- A YAML-driven admin tool that reads schema from disk.
- A Rhai callback running in an editor that filters rows by a string column name.

For those cases you need erasure. The price of erasure is that values become `CborValue` and ids
become `String` at the boundary; the type system from Step 1 doesn't propagate any further. That's
fine — generic code is rendering values, not deserialising into structs.

### Cargo wiring

The bridge is opt-in via a `vista` feature so non-Vista users don't transitively pull in
`vantage-vista`:

```toml
# in your backend's Cargo.toml
[features]
default = []
vista = ["dep:vantage-vista"]

[dependencies]
vantage-vista = { path = "../vantage-vista", optional = true }
```

Everything Vista-related — the factory module, the source, the YAML extras — sits under
`#[cfg(feature = "vista")]`. The `TableSource` path is unaffected, and downstream crates compile
without `vantage-vista` in their tree.

### File layout

Both in-tree drivers (CSV and MongoDB) converged on the same shape:

```
<driver>/src/vista/
├── mod.rs       re-exports + <Driver>::vista_factory() inherent impl
├── spec.rs      <Driver>TableExtras / <Driver>ColumnExtras / <Driver>VistaSpec
├── factory.rs   <Driver>VistaFactory + impl VistaFactory + spec→table helpers
├── source.rs    <Driver>TableShell + impl TableShell
└── cbor.rs      native ↔ CBOR bridge (only when native value type ≠ JSON-shaped)
```

CSV doesn't have `cbor.rs` — `From<AnyCsvType> for CborValue` already lived in the type-system
module for the `AnyTable` path, and the source reuses it. MongoDB's `bson::Bson` needs a richer
bridge (ObjectId, DateTime, Timestamp, Decimal128 each have lossy paths) so it gets a dedicated
file. Pick whichever fits — the trait shape doesn't change either way.

### The factory

Drivers expose a `vista_factory()` inherent method on the data source struct, so users construct
factories without naming an extra type:

```rust
impl YourDB {
    pub fn vista_factory(&self) -> YourVistaFactory {
        YourVistaFactory::new(self.clone())
    }
}
```

The factory struct holds whatever connection state it needs, plus two entry points and a trait
impl:

```rust
pub struct YourVistaFactory { db: YourDB }

impl YourVistaFactory {
    pub fn new(db: YourDB) -> Self { Self { db } }

    /// Typed entry point — kept off the `VistaFactory` trait to avoid making
    /// vantage-vista depend on vantage-table.
    pub fn from_table<E>(&self, table: Table<YourDB, E>) -> Result<Vista>
    where E: Entity<AnyYourType> + 'static
    { /* ... */ }
}

impl VistaFactory for YourVistaFactory {
    type TableExtras = YourTableExtras;
    type ColumnExtras = YourColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: YourVistaSpec) -> Result<Vista> { /* ... */ }
}
```

The `from_table` method is **inherent**, not on the trait. That's deliberate: putting it on the
trait would force `vantage-vista` to depend on `vantage-table`, which would couple the two crates
unnecessarily. Drivers want both, of course — but the universal Vista crate doesn't.

#### One source, two paths

The two construction paths must converge on identical source-creation code. Here's the pattern from
`MongoVistaFactory`:

```rust
pub fn from_table<E>(&self, table: Table<YourDB, E>) -> Result<Vista>
where E: Entity<AnyYourType> + 'static
{
    let name = table.table_name().to_string();
    let any_table = table.into_entity::<EmptyEntity>();
    let column_paths = paths_from_table_columns(&any_table);   // typed → paths
    Ok(self.wrap(any_table, column_paths, name))
}

fn build_from_spec(&self, spec: YourVistaSpec) -> Result<Vista> {
    let column_paths = self.paths_from_spec(&spec)?;            // YAML → paths
    let table = self.table_from_spec(&spec)?;
    Ok(self.wrap(table, column_paths, spec.name))
}

fn wrap(&self, table: Table<YourDB, EmptyEntity>, column_paths: ..., name: String) -> Vista {
    // single Vista::new call site — capability flags, source construction
}
```

The two paths only differ in *where they get their inputs from* — column metadata, the path map,
the table itself. Once the inputs are gathered, they go through the same `wrap` helper. That means a
future capability flip (advertising `can_subscribe`, say) is a one-line edit, not two. Drift between
the two construction paths is the most common Vista bug; this pattern is what keeps it out.

#### Two boundary details that bite

Two more details that look incidental but trip every driver:

**The vista's display name comes from `spec.name`, not the underlying table name.** A spec called
`client` mapped to a Mongo `clients` collection should expose `vista.name() == "client"` — that's
what UIs label their tabs with. The pattern: build the table from the spec (it gets the
collection/file/table name), wrap it via the same code as `from_table` (which sets the vista's name
from the table), then call `vista.set_name(spec.name)` to override. CSV's factory does this in
`build_from_spec` with one extra line; the typed `from_table` path doesn't need it because there
is no separate spec name.

**Resolve the id column in a fixed order**: explicit `spec.id_column` first, then the first column
flagged with `id`, then a backend default (`"_id"` for Mongo, `"id"` for most SQL, whatever your
backend's idiom is). Both in-tree drivers ship a `resolve_id_column` helper following exactly this
order. Don't reverse it — `spec.id_column` overrides flags is the rule that lets a YAML author
correct a bad column flag without editing the schema source.

### Harvesting metadata from a typed table

The typed entry path needs to project the typed table's columns into vista's universal column
metadata. Both in-tree drivers ship a near-identical helper:

```rust
fn metadata_from_table<T, E>(table: &Table<T, E>) -> VistaMetadata
where
    T: TableSource,
    E: Entity<T::Value>,
    T::Column<T::AnyType>: ColumnLike<T::AnyType>,
{
    let mut metadata = VistaMetadata::new();
    for (name, col) in table.columns() {
        let mut vc = VistaColumn::new(name.clone(), col.get_type().to_string());
        if col.flags().contains(&ColumnFlag::Hidden) {
            vc = vc.with_flag(vista_flags::HIDDEN);
        }
        metadata = metadata.with_column(vc);
    }
    if let Some(id) = table.id_field() {
        metadata = metadata.with_id_column(id.name().to_string());
    }
    for title in table.title_fields() {
        if let Some(col) = metadata.columns.get_mut(title) {
            col.flags.push(vista_flags::TITLE.to_string());
        }
    }
    metadata
}
```

The helper is generic enough to live in `vantage-vista` itself, but the in-tree drivers keep their
own copy. The reason is that `metadata` is *just* the universal projection — the moment you start
adding driver-specific column attributes (Mongo's BSON path, CSV's header alias) the helper has to
diverge. Keeping it next to the factory leaves room for that divergence without a refactor.

### The source

`TableShell` is the trait your executor implements. It mirrors `TableSource` in spirit — most
methods take `&Vista` so the source can read the current condition state, columns, and metadata —
but the value carrier is `CborValue` and the id is `String`:

```rust
#[async_trait]
impl TableShell for YourTableShell {
    async fn list_vista_values(&self, _vista: &Vista)
        -> Result<IndexMap<String, Record<CborValue>>>
    { self.read_all().await }

    async fn get_vista_value(&self, _vista: &Vista, id: &String)
        -> Result<Option<Record<CborValue>>>
    { /* ... */ }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        let condition = /* translate (field, value) → native condition */;
        self.table.add_condition(condition);
        Ok(())
    }

    fn capabilities(&self) -> &VistaCapabilities { &self.capabilities }
}
```

Two boundary conventions you must honour:

- **Ids stringify at the boundary.** Mongo's `ObjectId` becomes its 24-char hex; SurrealDB's `Thing`
  becomes its `table:id` form; AWS composite keys become whatever stable string they round-trip
  through. Inside the source you parse the string back to the native id type. `MongoTableShell`'s
  `parse_id` is one line — `MongoId::from_str(id)` with a `String` fallback so non-hex ids still
  flow through the same call.

- **Values translate to CBOR at the boundary.** Drivers with already-JSON-shaped values
  (CSV strings, REST JSON) reuse their existing `Into<CborValue>` impls. Drivers with richer native
  values (BSON, Surreal CBOR) need a dedicated bridge. The `cbor.rs` module is where lossy paths
  live — flag them in module docs, write round-trip tests for the lossless ones, and document the
  rest. Mongo's `bson_to_cbor` collapses `ObjectId`, `DateTime`, `Decimal128`, `Regex`,
  `JavaScriptCode`, and `Symbol` to strings; consumers wanting the native types back need to go
  through `Table<T, E>` directly.

  Two non-obvious conventions: on the way in (CBOR → native), unwrap `CborValue::Tag(_, inner)` and
  drop the tag — the inner value is what the backend stores. And integers wider than your native
  signed 64-bit type (`i128`, big BigInts) should stringify rather than silently truncate. Mongo's
  `cbor_to_bson` does both. Keep the bridge module-private (`pub(crate)` at most); a leaking BSON
  ↔ CBOR conversion gets called from places that should be going through `Vista` instead.

### Capabilities — the honesty contract

`VistaCapabilities` is six booleans plus a `PaginateKind`. They're the contract a generic UI relies
on to decide which buttons to draw:

```rust
VistaCapabilities {
    can_count: true,
    can_insert: true,
    can_update: true,
    can_delete: true,
    can_subscribe: false,
    can_invalidate: false,
    paginate_kind: PaginateKind::None,
}
```

Set a flag to `true` *only* if you actually override the matching `TableShell` method. Default
trait impls return `default_error(method, capability, vista)`, which produces one of two error
kinds:

- Flag is `false` → `ErrorKind::Unsupported` ("backend doesn't claim to do this; caller should have
  checked capabilities first").
- Flag is `true` → `ErrorKind::Unimplemented` ("backend advertised support but didn't override the
  method — placeholder bug").

This is the **lie detector**. If a UI sees `can_insert: true` and calls `insert_value`, it must
either succeed or fail with a real driver error — never a "you advertised this but didn't ship it"
placeholder. CSV illustrates the read-only end of this: `can_count: true` and everything else
`false`, so writes return `Unsupported` and the test asserts the kind explicitly.

### Conditions delegate; they never live on Vista

This is the design decision that made everything else click into place, and it's worth dwelling on
because the original plan had it backwards.

`Vista::add_condition_eq(field, CborValue)` delegates straight to
`TableShell::add_eq_condition(&mut self, field, value)`. The source translates the pair into the
driver's native condition type and pushes it onto the wrapped `Table`'s condition list. Vista
itself stores no condition state.

```rust
fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
    // CSV: build an Expression<AnyCsvType> via the operation trait
    let column = self.table.columns().get(field)
        .ok_or_else(|| error!("Unknown column for eq condition", field = field))?
        .clone();
    let csv_value: AnyCsvType = value.clone().into();
    self.table.add_condition(column.eq(csv_value));
    Ok(())
}

// Mongo: build a bson::Document with dot-notation for nested fields
fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
    let dotted = self.dotted_path(field);
    let bson_value = cbor_to_bson(value);
    self.table.add_condition(doc! { dotted: bson_value });
    Ok(())
}
```

#### Why not filter in memory after the fetch?

Because that defeats the database. A REST source pulling 50,000 rows over the wire to discard 49,990
in-memory is not a useful product. Every backend that supports server-side filtering — and that's
all of them — benefits from push-down. SQL drivers translate to `WHERE`, Mongo to `find` filter,
REST to query parameters, AWS to DynamoDB filter expressions. The universal CBOR pair is the lowest
common denominator that drivers translate up from.

#### Why `&CborValue` and not a typed value?

Because at the Vista boundary the Rust type isn't known. The caller is a CLI parsing a string
argument, or a YAML field, or a Rhai script. CBOR is the carrier; the driver decides how to project
it onto its native type. CSV's `From<CborValue> for AnyCsvType` and Mongo's `cbor_to_bson` are the
two halves of that translation in the in-tree drivers.

### Nested fields: the `column_paths` pattern

The MongoDB rollout surfaced a problem that any document-shaped backend will hit eventually: how do
you let a column called `city` in the spec map to `address.city` in the underlying document?

The answer is `column_paths: IndexMap<String, Vec<String>>` — a per-source map from spec column
name to BSON path segments. The source uses it three ways:

- **On read**, walk the path through the raw document and project the value out under the spec
  name.
- **On write**, rebuild intermediate sub-documents so `{ "address.city", "address.zip" }` lands as
  one `address: { city, zip }` BSON entry.
- **On filter**, join the path with `.` so Mongo can use the index server-side.

The path map is computed once at construction. Typed-table sources read column aliases (single-level
renames, since `with_alias` doesn't carry dotted paths). YAML-driven sources read each column's
`mongo: { nested_path: "address.city" }` block. Both feed the same `wrap` helper, so reads, writes,
and filters all see the same translation.

This is the second piece of accumulated wisdom from the rollout (alongside the eq-condition
delegation). Document-shaped backends — Surreal nested objects, REST JSON paths, AWS attribute maps
— should reuse the pattern. SQL backends won't need it: their column-to-field mapping is already
flat, and aliases ride on `with_alias`.

### YAML extras: three associated types, one `deny_unknown_fields`

Driver-specific YAML lives under three associated types on `VistaFactory`:

```rust
pub trait VistaFactory: Send + Sync + 'static {
    type TableExtras: Serialize + DeserializeOwned + Default + Send + Sync + 'static;
    type ColumnExtras: Serialize + DeserializeOwned + Default + Send + Sync + 'static;
    type ReferenceExtras: Serialize + DeserializeOwned + Default + Send + Sync + 'static;
    /* ... */
}
```

Each defaults to `NoExtras` for drivers with no driver-specific blocks. The convention is a
top-level key named after the driver — `csv:`, `mongo:`, `surreal:` — and the same key inside each
column entry:

```yaml
name: client
columns:
  _id:
    type: object_id
    flags: [id]
  full_name:
    type: string
    flags: [title]
    mongo:
      field: fullName        # column-level extras under `mongo:`
  city:
    type: string
    mongo:
      nested_path: address.city
mongo:
  collection: clients         # table-level extras under `mongo:`
```

Set `#[serde(deny_unknown_fields)]` on every extras struct. The outer `VistaSpec` can't (it uses
`#[serde(flatten)]` to merge the driver block in), but each *driver-owned* block must reject
typos — otherwise `mongo: { collctiom: clients }` silently falls back to defaults and you're
debugging a missing collection at runtime instead of at parse time.

Make every field inside the block `Option<T>` with `#[serde(default)]`, so the entire block can be
omitted when the spec name is enough. CSV's `csv: { path }` is mandatory (no path means no file);
Mongo's `mongo: { collection }` is optional and falls back to `spec.name`. Pick the convention that
fits your backend, but lean *omit* over *required* — YAML authors hate writing the same name twice.

Treat YAML errors as *parse errors*, not runtime errors. Validate paths, reject empty segments,
reject mutually-exclusive options up-front in `build_from_spec` and friends. The Mongo driver's
`MongoColumnBlock::resolved_path` is a worked example — it errors on empty `nested_path`, on
`a..b` style paths, and on empty `field` — woven with the column name so the YAML author can find
the bad entry.

### Tests

Vista tests are gated on `feature = "vista"` and run against the **real** backend, not a mock —
same as the `TableSource` tests in earlier steps. Use `Result<(), Box<dyn Error>>` so `?` covers
both your driver's native error type and `vantage_core::Error` uniformly:

```rust
#![cfg(feature = "vista")]

type TestResult = std::result::Result<(), Box<dyn Error>>;

#[tokio::test]
async fn vista_lists_typed_as_cbor() -> TestResult {
    let (db, name) = setup().await;          // fresh randomised database
    let table = product_table(db.clone());
    /* seed a row */
    let vista = db.vista_factory().from_table(table)?;

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);
    /* assert CborValue shapes */

    teardown(&db, &name).await;
    Ok(())
}
```

Ship two layers. Cheap unit tests (no backend needed) cover:

- **The CBOR bridge** — round-trip every native variant through CBOR and back. Scalars, nested
  maps, nested arrays. Lossy variants assert the documented lossy form rather than equality.
- **YAML parsing** — a minimal spec parses, an unknown field in the driver block errors loudly,
  optional blocks can be omitted.

Gated integration tests against the real backend then cover:

- **Typed `from_table` round-trip** — list, get-by-id, count match the seeded data.
- **YAML `from_yaml` round-trip** — same, plus the spec name overrides the underlying table name.
- **`add_condition_eq` push-down** — the count and list both honour the filter, and a second
  condition stacks via AND.
- **Capability advertisement** — the booleans match what the driver actually overrode. Read-only
  drivers must assert `can_insert: false` etc.
- **Read-only error kinds** — for an unsupported op, assert `ErrorKind::Unsupported` *and* that the
  message mentions the capability name. This is what catches the "advertised but unimplemented"
  drift.
- **Write round-trip via CBOR** (writeable drivers only) — insert, get, delete via the spec column
  names; verify the raw underlying document has the native shape (e.g. nested sub-doc rather than
  flattened keys).
- **Nested-path read/write/filter** (drivers using `column_paths`) — the most subtle of the lot,
  since it's where read, write, and filter must agree on the path map.

### Sharp edges

A few things bite every driver. They're worth flagging up front rather than discovering during
review.

**Id translation is the most common bug.** Round-trip your native id through `String` and back.
Assert that `vista.get_value(&id)` finds what `vista.list_values()` returned the id for — using
the *same* string, not a re-parsed one. Mongo's `MongoId::from_str` falling back to `String` for
non-hex inputs is the kind of asymmetry that hides until production.

**Aliases at the table level may not survive.** `Column::with_alias` is honoured by some
`TableSource` impls and ignored by others when materialising records — Mongo's `doc_to_record`
ignores them, which is why MongoDB's vista layer routes single-level renames through
`column_paths` instead. Audit your read path before relying on aliases for column renames; if the
table layer doesn't honour them, do the renaming in the vista source.

**Cursor-only backends should not advertise offset.** `PaginateKind` is a UI hint as much as a
declaration; getting it wrong means the UI offers an offset slider that never works. If the
backend is genuinely cursor-only (DynamoDB, many REST APIs), say so, and let consumers reject the
pagination shape they can't render.

**Conditions stay driver-typed for now.** Universal/portable conditions are a later stage. Until
then, only translate `eq`, and reject the rest at construction with a clear message — "operator
`Lt` not yet supported on `<DriverTableShell>`" beats a silent fall-back to in-memory filtering
every time.

**Capability flags are cheap to flip later.** Start narrow. A driver that ships with
`can_subscribe: false` and turns it on the day LIVE-query lands is a healthier state than one that
flagged it `true` in week one and has been shipping `Unimplemented` errors ever since.

### Step 7 conclusion

At this point your backend should have:

1. **A `vista` cargo feature** gating the bridge so non-Vista users don't pull in `vantage-vista`.

2. **`<Driver>::vista_factory()`** — inherent method on the data source returning a
   `<Driver>VistaFactory`.

3. **`<Driver>VistaFactory`** with two entry points and a trait impl:
   - `from_table<E>(Table<YourDB, E>) -> Result<Vista>` (inherent, typed path).
   - `impl VistaFactory` with `build_from_spec` (YAML path).
   - Both routing through one `wrap` helper that calls `Vista::new` exactly once.

4. **`<Driver>TableShell`** implementing `TableShell`:
   - Read methods translate native ids → `String` and native values → `CborValue` at the boundary.
   - Write methods (where supported) translate the other way.
   - `add_eq_condition` pushes a native condition onto the wrapped `Table`.
   - `capabilities()` returns a `VistaCapabilities` whose `true` flags exactly match the methods
     you actually overrode.

5. **YAML extras** under `spec.rs`:
   - `<Driver>TableExtras` and `<Driver>ColumnExtras` — both `deny_unknown_fields`.
   - `<Driver>VistaSpec` type alias resolving the three associated types.
   - Up-front validation of paths, mutual-exclusion rules, etc., as part of spec lowering.

6. **Tests** in `tests/<n>_vista.rs` — gated on `feature = "vista"`, run against a real backend,
   covering typed/YAML construction, read, write (where supported), `add_condition_eq` push-down,
   capability advertisement, and the `Unsupported` vs `Unimplemented` error-kind boundary.

Once Vista's wired up, the same generic CLI, admin UI, or scripting layer that already drives CSV
and MongoDB drives your backend too — without recompiling, without an entity import, without a
single backend-specific line of code on the consumer side. That's what the six previous steps were
clearing the runway for.
