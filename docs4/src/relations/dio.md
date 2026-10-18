# Relations and Dio

Everything in this guide so far has executed at or below the Vista: typed relations on the Table,
traversal narrowing, subquery expressions, implicit references. This final chapter climbs one layer
higher. A `Dio` keeps a live local copy of a data segment (a cache), fills it through a `Lens`,
announces changes on an event bus, and serves reactive views (`Scenery`). You built one in the intro
paths ([Dio over SQL](../intro/step5-sql-dio.md), [Scenery](../intro/step7-scenery.md)).

Two questions matter here: what happens to the relation machinery you already know once a Dio sits
on top — and what relation shape exists *only* at the Dio layer, because nothing below it can
express one.

### Same-persistence relations resolve beneath the Dio

Traversal, subquery expressions, and implicit references all happen in the Table or Vista *under*
the Dio. The Dio caches the resulting rows like any others. If your table declares
`with_active_columns(&["id", "client.name"])`, the `client.name` column is already part of each row
when it enters the cache — nothing at the Dio layer knows or cares that it was traversed. From the
cache's point of view, a traversed column and a plain column are indistinguishable.

The practical consequence is the master/detail pattern in a UI: **traverse at the Vista, cache at
the Dio**. Take a row from the cached master and call `Dio::get_ref(relation, &row)`. It runs the
erased `Vista::get_ref` from the [Vistas chapter](./vistas.md) on the Dio's master and builds a new
Dio over the narrowed target, on the same Lens:

```rust
# use ciborium::Value as CborValue;
# use vantage_dataset::ReadableValueSet;
# use vantage_diorama::{Cache, Lens, Loader};
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::{Column, JoinKey, Reference, Vista, VistaMetadata};
#
# fn row(pairs: &[(&str, &str)]) -> Record<CborValue> {
#     pairs.iter().map(|(k, v)| (k.to_string(), CborValue::Text(v.to_string()))).collect()
# }
#
# fn clients() -> Vista {
#     let orders = MockShell::new()
#         .with_metadata(
#             VistaMetadata::new()
#                 .with_column(Column::new("id", "String").with_flag("id"))
#                 .with_column(Column::new("client_id", "String"))
#                 .with_id_column("id"),
#         )
#         .with_record("o1", row(&[("id", "o1"), ("client_id", "marty")]))
#         .with_record("o2", row(&[("id", "o2"), ("client_id", "marty")]))
#         .with_record("o3", row(&[("id", "o3"), ("client_id", "doc")]));
#     let shell = MockShell::new()
#         .with_metadata(
#             VistaMetadata::new()
#                 .with_column(Column::new("id", "String").with_flag("id"))
#                 .with_id_column("id")
#                 .with_reference(Reference::has_many("orders", "order", JoinKey::new("id", "client_id"))),
#         )
#         .with_record("marty", row(&[("id", "marty")]))
#         .with_record("doc", row(&[("id", "doc")]))
#         .with_ref_target("orders", orders);
#     Vista::new("client", shell)
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let lens = Lens::builder()
    .cache(Cache::memory())
    .loader(Loader::eager(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    }))
    .build()?;

let clients = lens.dio(clients()).build().await?;
let marty = clients.cached().get_value("marty").await?.expect("cached");

// A Dio of its own over marty's orders, with its own cache table.
let orders = clients.get_ref("orders", &marty).await?;
assert_eq!(orders.cached().list_values().await?.len(), 2);
# Ok(())
# }
```

Each narrowed detail set gets its own cache table, keyed by the narrowed target's query, so
switching between master rows switches between already-cached detail sets rather than re-narrowing
one shared cache. The detail Dio inherits the master's live setting, but not its augmentations,
which describe the master's table. `Dio::get_ref_target(relation)` is the same call without a row:
a Dio over every eligible target row, which is what a reference picker in an edit form lists.

When the detail lives in another datasource, build its Vista through a
[`Catalog`](vantage_vista::Catalog) and narrow it with `Reference::narrow`, as the
[Vistas chapter](./vistas.md#across-datasources-the-catalog) shows, then put a Dio in front of it
with `lens.dio(narrowed).build()`.

A narrower view of the *same* rows is a different operation: no relation, just more conditions.
`dio.derive()` collects them and builds a new Dio over the narrowed set:

```rust
# use ciborium::Value as CborValue;
# use vantage_dataset::ReadableValueSet;
# use vantage_diorama::{Cache, Lens, Loader};
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::{Column, Condition, FilterOp, Vista, VistaMetadata};
#
# fn row(pairs: &[(&str, &str)]) -> Record<CborValue> {
#     pairs.iter().map(|(k, v)| (k.to_string(), CborValue::Text(v.to_string()))).collect()
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let orders = Vista::new(
#     "order",
#     MockShell::new()
#         .with_metadata(
#             VistaMetadata::new()
#                 .with_column(Column::new("id", "String").with_flag("id"))
#                 .with_column(Column::new("client_id", "String"))
#                 .with_id_column("id"),
#         )
#         .with_record("o1", row(&[("id", "o1"), ("client_id", "marty")]))
#         .with_record("o2", row(&[("id", "o2"), ("client_id", "doc")])),
# );
# let lens = Lens::builder()
#     .cache(Cache::memory())
#     .loader(Loader::eager(|cx| async move {
#         let rows = cx.master().list_values().await?;
#         cx.reconcile(rows).await
#     }))
#     .build()?;
# let dio = lens.dio(orders).build().await?;
let martys = dio
    .derive()
    .with_condition(Condition::new("client_id", FilterOp::Eq, "marty"))
    .build()
    .await?;
assert_eq!(martys.cached().list_values().await?.len(), 1);
# Ok(())
# }
```

`martys` keeps a cache table and an event bus of its own, but it isn't cut off from `dio`. Every
Dio over the `order` table on this Lens joins the table's hub. What one of them settles (a
confirmed flash, an `apply_change`, what a reconcile changed, a change its live follow received)
reaches the others, and each judges it against its own conditions. Move `o1` to doc through `dio`
and it leaves `martys`. Insert an order for marty and it shows up in `martys`. A row that only left
a narrowed Dio's set is never taken as a delete: the others re-read it through their own masters.

The hub is keyed by the master's driver and name. A layer that renames vistas, one name per
session, passes the backend table to `DioBuilder::hub(key)` so its Dios still hear each other. An
explicit key also names the cache table, so Dios keyed apart never share cached rows. A master
narrowed by a rule its `query()` doesn't record (a table hook's) is built with
`DioBuilder::recheck_facts()`, which re-reads each reported row through that master rather than
judging it in memory.

`recheck_facts` keeps nothing apart. Dios are told apart by vista name, recorded query and hub key
only, so a hidden per-session rule needs its own vista name, a vista condition, or its own hub key.
Without one, two sessions share a cache table and each can list the other's rows.

### The join the layers below cannot express

A [`Catalog`](./vistas.md#across-datasources-the-catalog) can already *navigate* across
datasources — hand a `Reference` a parent row and the catalog's target, and it returns the related
set from another backend. What nothing below the Dio can do is *merge*: show master rows whose
columns come from two sources at once. That is a join, and joins are governed by the capability
contract — a SQL Vista can join its own tables, a REST Vista cannot, and two different backends can
never push a join down to either engine. A Vista also has nowhere to put stitched rows: no cache,
no viewport, no notion of "only visible rows pay".

Where the backend can't, the layer above fills the gap — that is the whole job of the Dio.
**Augmentation wires two Vistas into one `Dio`**: a *master* that is listed, and a *detail* source
loaded one row at a time and merged on top. The detail is resolved by name through a
[`VistaResolver`](vantage_vista::VistaResolver) such as a `Catalog`, so it is
persistence-agnostic — a REST master enriched by a `cmd` detail, or a SQL master enriched from
another database.

It runs in two passes:

1. **List pass** — the master is listed cheaply; each row enters the cache marked `Incomplete`.
2. **Detail pass** — viewport-driven: for each *visible* incomplete row, the augmentation resolves
   its detail Vista, fetches the matching record, merges the chosen columns, and the row flips to
   `Fresh`.

Rows off-screen are never fetched; hydrated rows are never re-fetched. A failed detail fetch marks
only that row.

### Declaring an augmentation

Augmentations are a property of the Dio, fixed when it is built: `DioBuilder::augment` takes the
list, and supplying at least one is what engages the two-pass behaviour. Each augmentation answers
four questions — which detail Vista, how a master row selects its detail, how it is fetched, and
which columns to lift:

```rust
# use std::sync::Arc;
#
# use vantage_diorama::{Augmentation, Cache, Detail, Fetch, Lens, MergeRule, Source};
# use vantage_vista::mocks::MockShell;
# use vantage_vista::{Catalog, Column, Vista, VistaMetadata};
#
# fn meta(columns: &[&str]) -> VistaMetadata {
#     columns
#         .iter()
#         .fold(VistaMetadata::new(), |m, c| m.with_column(Column::new(*c, "String")))
#         .with_id_column("id")
# }
#
# fn tfstate_detail() -> Vista {
#     Vista::new("tfstate_detail", MockShell::new().with_metadata(meta(&["id", "resources", "serial"])))
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let buckets = Vista::new("buckets", MockShell::new().with_metadata(meta(&["id", "key", "size"])));
# let lens = Lens::builder().cache(Cache::memory()).build()?;
let catalog = Catalog::new().with("tfstate_detail", |_: &str| Ok(tfstate_detail()));

let dio = lens
    .dio(buckets)
    .augment(vec![Augmentation::new(
        Detail::Resolved {
            resolver: Arc::new(catalog),     // resolves the detail by name
            name: "tfstate_detail".into(),
        },
        Source::Column { from: "key".into(), to: None },
        Fetch::PerRow,
        MergeRule::lift(["resources", "serial"]),
    )])
    .build()
    .await?;
# let _ = dio;
# Ok(())
# }
```

- `detail` names the detail Vista: `Detail::Resolved` looks it up by name through a resolver;
  `Detail::Fixed(Arc<Vista>)` takes one directly, for a side table no resolver knows.
- `source` picks how a master row selects its detail: `Source::Id` matches master.id → detail.id;
  `Source::Column { from, to }` matches `master[from]` → `detail[to]`, or detail.id when `to` is
  `None`; `Source::Build(closure)` performs arbitrary narrowing from the whole row, and is per-row
  only.
- `fetch` picks how the detail is read: `Fetch::PerRow` reads one record per master row;
  `Fetch::Custom(closure)` is a caller-supplied async fetch.
- `merge` lists the detail columns to lift. `MergeRule::lift_all()` lifts all of them; on a name
  clash the detail wins.

Diorama has no YAML form of these types. A consumer maps its own configuration onto them:
vantage-ui reads an `augment:` block from a table's configuration and builds the same
`Augmentation`:

```yaml
augment:
  - table: tfstate_detail
    source: { kind: column, from: key }
    fetch:  { kind: per_row }
    merge:  [resources, serial, outputs]
```

```admonish info title="Script sources"
A scripted source is a `Build` closure the consumer writes; diorama never sees a script string.
See [Augmentation](../augmentation.md#rhai-as-a-closure-factory).
```

To see the two passes run, try the example — two in-memory Vistas, list pass then detail pass:

```console
cargo run -p vantage-diorama --example augmentation
```

The full treatment of augmentation — merge rules, capability reasoning, the planned batched fetch
and keyed caching — is in the [Augmentation chapter](../augmentation.md); the tutorial form is
[intro step 6](../intro/step6-augmentation.md).

### Why not model the detail as a foreign-key relation?

A relation resolves *within* one persistence and can be pushed down as a join. The detail here may
be a different backend entirely; there is nothing to join on and no engine that could honour it.
Augmentation is the cross-Vista form, stitched by the Dio.

### Choosing between implicit references and augmentation

Both produce the same user-visible result: a row enriched with related fields. Which one to use
depends on where the data lives and who should pay for the fetch.

#### Path A: implicit references (same datasource, in-query)

One query; the backend does the work. The value is present on every row, including off-screen ones,
it is validated when the table is constructed, and it carries read-only column semantics. Choose
this when both tables live in one datasource that supports traversal (SQL, SurrealDB, MongoDB) —
see [Implicit References](./implicit-references.md).

#### Path B: augmentation (any two datasources, per-row client-side join)

Two passes; expensive work follows the user's attention — only viewport rows pay, and only once.
It works across arbitrary backends, and failed details degrade per-row, not per-page. Choose this
when the sources differ, or when the detail is expensive and you want viewport-driven hydration
even within one backend.

The key difference: implicit references push the join *down* into the engine; augmentation lifts it
*up* into the cache. Same user-visible result — a row enriched with related fields — chosen by where
the data lives and who should pay for the fetch.

### Guide conclusion

This closes the Relations & Traversal guide. At this point you should be able to:

1. **Say where each relation form executes** — traversal and implicit references in the engine,
   erased traversal through the Vista handle, augmentation in the Dio's cache.
2. **Build master/detail** — `dio.get_ref(relation, &row)` for a Dio over the related rows, each
   detail set in its own cache table; `dio.get_ref_target(relation)` for every eligible row.
3. **Narrow a Dio** — `dio.derive().with_condition(..).build().await?` for a narrower view of the
   same rows.
4. **Declare an augmentation** — `Detail::Resolved` or `Fixed`, a `Source`, a `Fetch` and a
   `MergeRule`, handed to `lens.dio(master).augment(..)` before `build()`.
5. **Choose deliberately** between implicit references and augmentation based on where the data
   lives and who should pay for the fetch.

Relations in Vantage are declared once on the model and then travel — into queries, through
erasure, and up into the cache — each layer serving the form the one below can't.
