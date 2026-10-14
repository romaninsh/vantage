# References, catalog and rhai

1.x had five ways to name "something that gives me a Vista for a name": `TargetResolver`,
`ModelResolver`, `ModelLoader`, `ContainedRefResolver` and each driver's `*SpecResolver` alias. It
had two relation types, vantage-vista's `Reference` and vantage-vista-factory's `Relation`, and a
crate whose only job was a registry of loaders. 2.0 keeps one of each, following rule 1: one
`VistaResolver` trait, one `Reference`, and a `Catalog` inside vantage-vista. The factory crate is
deleted with no shim (decision 6), so each import fails and points here. The rhai layer follows
rule 11: its items live under `vantage_vista::rhai` only, and its script verbs match the Rust
ones. Read [Relations: Vista, YAML and Rhai](../relations/vistas.md) and
[Config-Driven Vistas](../config-driven-vistas.md) for the current shape.

## Reference constructors

`Reference::new(name, target, kind, keys)` and `Reference::contained(name, kind, host)` become one
constructor per kind. A join can't be built without a key, and a contained kind can't carry one.

```rust,compile_fail,E0599
# use vantage_vista::{JoinKey, Reference, ReferenceKind};
let players = Reference::new("players", "players", ReferenceKind::HasMany, vec![JoinKey::new("id", "team_id")]);
```

```rust
# use vantage_vista::{JoinKey, Reference, ReferenceKind};
let players = Reference::has_many("players", "players", JoinKey::new("id", "team_id"));
let captain = Reference::has_one("captain", "players", JoinKey::to_target_id("captain_id"));
let kit = Reference::contains_many("kit", "kit_items");
# assert!(players.kind == ReferenceKind::HasMany && captain.kind == ReferenceKind::HasOne);
# assert!(kit.kind.is_contained());
```

A multi-key join comes from YAML `keys:` through `ReferenceSpec::to_reference(relation,
source_id)`; `with_key` is crate-private.

## The catalog folds into vantage-vista

`VistaCatalog` becomes `vantage_vista::Catalog`. A loader receives the name it was asked for, and
the catalog is a `VistaResolver`, so it goes anywhere a resolver is held. A name it doesn't hold is
an `is_not_found()` error.

```rust,compile_fail,E0432
# use std::sync::Arc;
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Catalog, Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# fn main() -> vantage_core::Result<()> {
use vantage_vista_factory::VistaCatalog;

let mut catalog = VistaCatalog::new();
catalog.register("teams", Arc::new(|| Ok(teams())));
let vista = catalog.build_vista("teams")?;
# Ok(())
# }
```

```rust
# use std::sync::Arc;
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Catalog, Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# fn main() -> vantage_core::Result<()> {
let catalog = Catalog::new().with("teams", |_: &str| Ok(teams()));
let vista = catalog.vista("teams")?;
# assert!(catalog.vista("players").is_err());
# let _ = vista;
# Ok(())
# }
```

A cross-model relation is a `Reference`, traversed as `reference.narrow(catalog.vista(&reference.target)?,
&row)?`. [Config-Driven Vistas](../config-driven-vistas.md#cross-persistence-the-catalog) has the
full example.

## One resolver trait

`TargetResolver` (`Arc<dyn Fn(&str) -> Result<Vista>>`) and api-client's `ModelResolver` become
`Arc<dyn VistaResolver>`. A closure still implements the trait when its parameter is typed
`&str`; a call `resolver(name)` becomes `resolver.vista(name)`.

```rust,compile_fail,E0432
# use std::sync::Arc;
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Catalog, Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# fn main() -> vantage_core::Result<()> {
use vantage_vista::rhai::TargetResolver;

let resolver: TargetResolver = Arc::new(|_: &str| Ok(teams()));
let vista = resolver("teams")?;
# Ok(())
# }
```

```rust
# use std::sync::Arc;
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Catalog, Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# fn main() -> vantage_core::Result<()> {
let resolver: Arc<dyn VistaResolver> = Arc::new(Catalog::new().with("teams", |_: &str| Ok(teams())));
let vista = resolver.vista("teams")?;
# let _ = vista;
# Ok(())
# }
```

## The rhai host API

The rhai items leave the crate root for `vantage_vista::rhai`, and `DataVocab`'s four
constructors become `DataVocab::new(terminals).with_resolver(resolver)`. `Terminals` is `Describe`
or `ReadWrite`; a read-only host is `ReadWrite` with `Writes::Denied(message)`, so every write
throws that message.

```rust,compile_fail,E0432
# use std::sync::Arc;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::Catalog;
# fn main() {
# let resolver: Arc<dyn VistaResolver> = Arc::new(Catalog::new().with("teams", |_: &str| Ok(Vista::new("teams", MockShell::new()))));
use vantage_vista::{DataVocab, Terminals, Writes};

let vocab = DataVocab::read_write(Some(resolver));
# }
```

```rust
# use std::sync::Arc;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::Catalog;
# fn main() {
# let resolver: Arc<dyn VistaResolver> = Arc::new(Catalog::new().with("teams", |_: &str| Ok(Vista::new("teams", MockShell::new()))));
use vantage_vista::rhai::{DataVocab, Terminals, Writes};

let vocab = DataVocab::new(Terminals::ReadWrite { limit: None, writes: Writes::Allowed })
    .with_resolver(resolver.clone());
let read_only = DataVocab::new(Terminals::ReadWrite {
    limit: Some(100),
    writes: Writes::Denied("this host is read-only".into()),
})
.with_resolver(resolver);
# let _ = (vocab, read_only);
# }
```

| 1.x | 2.0 |
|---|---|
| `vantage_vista::{DataVocab, Handle, RecordDraft, run_script, …}` | `vantage_vista::rhai::{…}` |
| `vantage_diorama::rhai::{cbor_to_dynamic, dynamic_to_cbor, record_to_map}` | `vantage_vista::rhai::{…}` |
| `DataVocab::describe(r)`, `read(r, l)`, `read_write(r)`, `read_write_with(r, l, w)` | `DataVocab::new(Terminals::Describe \| ReadWrite { limit, writes }).with_resolver(r)` |
| `Terminals::Read { limit }` | `Terminals::ReadWrite { limit, writes: Writes::Denied(msg) }` |
| `Handle::over`, `steps`, `Step` | crate-private; `Handle::query() -> Result<VistaQuery>` |
| `Handle::with_base_vista(verb, FnOnce(&mut Vista))` | `Handle::with_native_condition(verb, NativeCondition)` |
| `Handle::resolve(Option<&TargetResolver>)`, `run_script(s, TargetResolver, ..)`, `preview_script` | `resolve(Option<&dyn VistaResolver>)`; the runners take `Arc<dyn VistaResolver>` |
| `RecordDraft::{new_row, load, from_row}(handle, resolver, ..)` | the resolver rides on the handle: [Drivers](./drivers.md#the-resolver-rides-on-the-handle) |
| `eval_augment_source`, `AugmentSourceFn`, `augment_source_closure`, `eval_lazy_expression`, `lazy_value_closure`, `LazyValueFn` | removed; `Column::with_expression`, or your own `Source::Build` closure |
| `vantage_rhai::register_constant` | `vantage_rhai::Namespace` (`ns.var(..)`, read as `ns::name`) |
| `reject_dropped_narrowing(ast, what)` | `reject_dropped_narrowing(ast, what, has_value)` |

## Script changes

Scripts change in four places: the record verb, statement chaining, capability keys, and the
backend query builders, which moved into namespaces so they can't shadow `table(name)`.

```rhai
let o = table("order").record("o2");
o.status = "paid";
o.save();
if table("order").capabilities().can_insert { /* … */ }
self.where("status", "open"); self.sort("total", "desc");
select().from("product").field(ident("name"))
```

```rhai
let o = table("order").record("o2");
o.status = "paid";
o.flash();
if table("order").capabilities().insert { /* … */ }
self.where("status", "open").sort("total", "desc")
sql::select().from("product").field(sql::ident("name"))
```

- **`save()` is `flash()`** on rhai records and on the servo (decision 5): it sends an insert, or a
  patch of the changed fields only. `save` is "Function not found".
- **A handle script's value is its last statement.** In `modify:`, augment and reference scripts,
  a statement that narrows a handle and drops it is an error when the script compiles; chain the
  calls or assign them. Watch `if c { self.where(..) }` with no `else`: end it with `else { self }`.
- **`capabilities()` keys come from `Capability::iter()`** (`insert`, `order`, `window`,
  `filter_ops`, …). The dropped `can_count`, `can_fetch_page`, `can_fetch_next`,
  `can_set_page_size`, `can_invalidate`, `can_traverse_*` and `can_build_ref_via_script` read as
  `()`, so an old script doesn't error; it reads unit. `count()` is always available.
- **Namespaced builders.** SQL functions are `sql::` for SQLite, PostgreSQL and MySQL; SurrealDB's
  are `surreal::`, and its `me` anchor is `surreal::me`; MongoDB's `oid`, `date`, `doc` are
  `mongo::`. The `table("x")` alias for `ident` is gone: write `sql::ident("x")`. Methods,
  indexers, operators and `with_condition` are unchanged.
- **`references()` reports the YAML kind names** (`has_one`, `has_many`, `contains_one`,
  `contains_many`), and `rejection()` is `#{ message }`.
- **Writes are gated by the Vista.** A denied or unsupported write throws (`delete` used to return
  `false`); `upsert` needs `Update` only; `import_from` needs `Import`; `limit(n)` reads
  `fetch_window(0, n)` with no fallback to a truncated `list()`.

## Renames

| 1.x | 2.0 |
|---|---|
| `Reference::contained(name, ReferenceKind::ContainsOne, host)` | `Reference::contains_one(name, host)` (`contains_many` likewise) |
| `reference.narrow(target, row, coerce)` | `reference.narrow(target, row)`; a driver re-tags ids in its own `add_condition` |
| `Reference::foreign_key()`, `ReferenceKind::default()` | read `reference.keys`; name the kind |
| `with_build_script(s) -> Self` | `-> Result<Self>`; refused on a contained kind |
| `ContainedSpec`, `ContainedKind`, the `ContainedYaml` lowering | `Reference` with a `Contains*` kind |
| `ContainedRefResolver` (keyed by relation name) | `build_contained_vista(.., Option<Arc<dyn VistaResolver>>)`, called with `reference.target` |
| `ModelLoader`, `VistaCatalog::register` | a `Catalog::with(name, loader)` loader |
| `VistaCatalog::{build_vista, has_model, model_names}` | `catalog.vista(name)` (`is_not_found()` for a missing name) |
| `Relation`, `Relation::{single_key, multi_key, narrow}`, `traverse`, `traverse_from` | `Reference`, `reference.narrow(catalog.vista(&reference.target)?, &row)`, or `parent.get_ref(relation, Some(&row))` on one backend |
| `vantage_diorama::prelude::VistaCatalog` | `vantage_vista::Catalog` |
| each shell's "reference not found" error | `vantage_vista::unknown_relation(relation)`, `is_not_found()` |

## Behaviour notes

- **A serialised `Reference` is validated**: a contained kind needs `host_column` and no `keys` or
  build script; a join needs at least one key and no `host_column`.
- **`Reference.target` is a name a catalog holds**: the spec name for YAML references, the built
  table's name for typed ones (it used to be a Rust type name), so `catalog.vista(&r.target)` works
  on every driver.
- **A contained record's resolver is called with `reference.target`** (`products`), not the
  relation name (`product`), so a `Catalog` can be passed as it is.
- **Strict joins.** Traversal narrows the target by the join's `to` column, so the target must
  declare it. See [Drivers](./drivers.md#declare-the-foreign-key-on-the-target).

## Checklist

- [ ] no `vantage_vista_factory` import; `Catalog::new().with(..)` and `catalog.vista(name)`
- [ ] every `Reference::new` / `Reference::contained` is a per-kind constructor
- [ ] resolvers are `Arc<dyn VistaResolver>`; closures type their parameter `|name: &str|`
- [ ] rhai items are imported from `vantage_vista::rhai`; `DataVocab::new(..).with_resolver(..)`
- [ ] scripts use `flash()`, chain their handle calls, read `capabilities()` by capability name,
      and call `sql::`, `surreal::` and `mongo::` builders
