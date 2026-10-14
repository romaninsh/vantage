# Dio and Lens

In 1.x a Lens had eight hooks, each handed a `&Dio`, and a Dio could be narrowed, re-pointed and
augmented after it was built, by anyone holding it. Rule 5 ended that: a Dio is a shared handle,
so `&self` never changes what another holder sees. Everything a Dio is (its master, its
augmentations, its live follow) is chosen on a builder before `build()`, and narrowing derives a
new Dio with its own cache identity. The Lens went the same way: four typed settings (`Loader`,
`Refresher`, `writer`, `Tuning`), and a `build()` that checks the combination and returns
`Result<Arc<Lens>>`. One query type, `VistaQuery`, flows through every load path, which is what
fixed paged loads that ignored conditions. The chapter to read alongside is
[A Facade over a Custom API](../intro/step5-dio-lens.md).

The Lens snippets use `copy`, an `async fn copy(cx: LoaderCx) -> Result<()>` that lists
`cx.master()` and hands the rows to `cx.reconcile`.

## The Lens builder

`Lens::new()` and `LensBuilder::new()` become `Lens::builder()`, and `build()` returns the Lens
already in an `Arc`. The hooks become typed settings: `on_start` is `Loader::eager`,
`on_load_chunk` is `Loader::paged`, `on_refresh` with `refresh_every` is `Refresher::new(..).every(..)`.
Every callback takes an owned `LoaderCx` instead of `&Dio`, so the `let dio = dio.clone()` dance
goes.

```rust,compile_fail,E0599
# use std::{sync::Arc, time::Duration};
# use vantage_diorama::{prelude::*, Cache, Loader, LoaderCx, Refresher};
# use vantage_vista::prelude::*;
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let lens = Arc::new(
    Lens::new()
        .cache_in_memory()
        .on_start(|dio: &Dio| {
            let dio = dio.clone();
            async move { dio.reconcile(dio.master().list_values().await?).await }
        })
        .refresh_every(Duration::from_secs(60))
        .build()?,
);
# Ok(())
# }
```

```rust
# use std::{sync::Arc, time::Duration};
# use vantage_diorama::{prelude::*, Cache, Loader, LoaderCx, Refresher};
# use vantage_vista::prelude::*;
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let lens: Arc<Lens> = Lens::builder()
    .cache(Cache::memory())
    .loader(Loader::eager(copy))
    .refresher(Refresher::new(copy).every(Duration::from_secs(60)))
    .build()?;
# Ok(())
# }
```

`build()` refuses a combination that would fail later, and names the setting in the error's
`setting` context: no cache, no tokio runtime, `Loader::blocking` without an eager start,
`or_eager` on a non-paged loader, `every(Duration::ZERO)`, a write queue or worker count of zero,
a redb file that won't open. `LensBuildError` is gone; match the context instead.

## Building a Dio

`make_dio(vista)` and `make_dio_as(vista, name)` become `lens.dio(vista)`, a builder, then
`.build().await?`. There is no name argument: the cache identity comes from the vista (below).
`build()` fails on a vista without an id column, naming the table, where 1.x fell back to `"id"`.

```rust,compile_fail,E0599
# use vantage_diorama::{prelude::*, Cache, Loader, LoaderCx};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let lens = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?;
let dio = lens.make_dio(teams()).await?;
# Ok(())
# }
```

```rust
# use vantage_diorama::{prelude::*, Cache, Loader, LoaderCx};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let lens = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?;
let dio = lens.dio(teams()).live().build().await?;
# assert_eq!(dio.cached().list_values().await?.len(), 2);
# Ok(())
# }
```

`.live()` replaces `dio.watch()`: the follow starts at build, a subscribe failure fails the build,
and over a master without `Subscribe` it does nothing. While a Dio with the same cache identity,
augmentations (the same `Arc`) and `live` setting is held, `build()` returns that Dio.

## Augmentation goes on the builder

`Dio::augment(resolver, augmentations)` after construction becomes `DioBuilder::augment(..)`, and
the resolver moves into each detail: `Detail::Catalog(name)` is `Detail::Resolved { resolver,
name }`. A `Fetch::Custom` closure now receives `(Vista, Record)`: the narrowed detail and a copy
of the listed row.

```rust,compile_fail,E0599
# use std::sync::Arc;
# use vantage_diorama::{prelude::*, Augmentation, Cache, Detail, Fetch, MergeRule, Source};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Catalog, Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let lens = Lens::builder().cache(Cache::memory()).build()?;
let labels: Arc<dyn VistaResolver> = Arc::new(Catalog::new().with("labels", |_: &str| Ok(teams())));
let label = Augmentation::new(Detail::Catalog("labels".to_string()), Source::Id, Fetch::PerRow, MergeRule::lift(["name"]));
let dio = lens.dio(teams()).build().await?;
dio.augment(labels, vec![label]).await?;
# Ok(())
# }
```

```rust
# use std::sync::Arc;
# use vantage_diorama::{prelude::*, Augmentation, Cache, Detail, Fetch, MergeRule, Source};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Catalog, Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let lens = Lens::builder().cache(Cache::memory()).build()?;
let labels: Arc<dyn VistaResolver> = Arc::new(Catalog::new().with("labels", |_: &str| Ok(teams())));
let label = Augmentation::new(
    Detail::Resolved { resolver: labels, name: "labels".into() },
    Source::Id,
    Fetch::PerRow,
    MergeRule::lift(["name"]),
);
let dio = lens.dio(teams()).augment(vec![label]).build().await?;
# let _ = dio;
# Ok(())
# }
```

## Narrowing derives a new Dio

`with_condition_eq` and the mutating `with_order` changed every holder's Dio. `dio.derive()` builds
a new one and leaves the original alone; clauses the master refuses, and clauses on augmented
columns, become the derived Dio's `residual()`. The phase-3 `Dio::with_query(&q)` is
`dio.derive().with_query(&q)`.

```rust,compile_fail,E0599
# use vantage_diorama::{prelude::*, Cache, Loader, LoaderCx};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
dio.with_condition_eq("name", "red").await?;
# Ok(())
# }
```

```rust
# use vantage_diorama::{prelude::*, Cache, Loader, LoaderCx};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
let red = dio
    .derive()
    .with_condition(Condition::new("name", FilterOp::Eq, "red"))
    .build()
    .await?;
assert_eq!(dio.diagnostics().await?.cache_table, "teams");
assert!(red.diagnostics().await?.cache_table.starts_with("teams?"));
# Ok(())
# }
```

The derived Dio's cache table is derived too: `name` when nothing narrows the vista, else
`name?<VistaQuery::key()>`, with `%`, `?` and control characters in the name percent-encoded.
That replaces `make_dio_as` names, `get_ref`'s `{target}-via-{relation}-{id}` tables and hand-built
`-where-` keys. A vista narrowed before `lens.dio(..)` gets its own table as well. Old cache files
keep their old tables, which nothing opens again; sweep them with `CacheBackend::list_tables` and
`drop_table` if the space matters.

## Reading through the Dio

`fetch_window_ordered(offset, limit, query)` and its `_counted` twin become `dio.fetch(window,
&query)`, returning `Fetched { page, refused }`. A clause the master refuses comes back in
`refused` instead of a `tracing::warn!` and a silent drop.

```rust,compile_fail,E0599
# use vantage_diorama::{prelude::*, Cache, Loader, LoaderCx};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
let (rows, total) = dio.fetch_window_ordered_counted(0, 10, &VistaQuery::new()).await?;
# Ok(())
# }
```

```rust
# use vantage_diorama::{prelude::*, Cache, Loader, LoaderCx};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
let fetched = dio.fetch(0..10, &VistaQuery::new()).await?;
let (rows, total) = (fetched.page.rows, fetched.page.total);
if !fetched.refused.is_empty() {
    // evaluate the refused clauses over a complete set, or refuse the read
}
# assert_eq!(rows.len(), 2);
# let _ = total;
# Ok(())
# }
```

## Renames

| 1.x | 2.0 |
|---|---|
| `cache_at(path)`, `cache_in_memory()`, `cache_source(b)` | `.cache(Cache::redb(path))`, `.cache(Cache::memory())`, `.cache(Cache::backend(b))` |
| `on_start(f)` + `on_start_blocking(b)`, `on_load_chunk(f)`, both | `Loader::eager(f).blocking(b)`, `Loader::paged(f)`, `Loader::paged(chunk).or_eager(start)` |
| `on_refresh(f)` + `refresh_every(d)`, `cache_ttl` | `Refresher::new(f).every(d)`, `Refresher::max_age(ttl)` |
| `on_flash(f)` | `.writer(f)`: [Writes and ingress](./writes-ingress.md) |
| `write_queue_capacity`, `viewport_debounce`, `augment_workers`, `refresh_on_open`, `LensDefaults` | `.tuning(Tuning::default().with_write_queue(n).with_viewport_debounce(d)…)` |
| `on_event`, `Dio::handle_event` | `Dio::apply_change` |
| `total_provider` | the paged loader calls `ChunkSink::set_total` |
| `on_list_page`, `on_load_detail`, `QueryDescriptor` | an `Augmentation` on `lens.dio(v).augment(..)` |
| `activity_signal`, `Activity`, `ActivitySignal`, `standby_refresh_every` | removed (decision 3) |
| `ChunkQuery` handed to a chunk loader | the view's `VistaQuery`, by value |
| `Dio::vista()` | `Dio::cached()`, the facade Vista |
| `Dio::cache()`, `Dio::cache_table_name()` | `LoaderCx::cache()` / `reconcile()` in callbacks; `dio.diagnostics().await?.cache_table` |
| `Dio::master() -> Arc<Vista>` | `-> &Vista`; also `residual()` and `id_column()` |
| `Dio::reload(vista)` | build a new Dio over the new vista |
| `LoaderCx::id_column()` | `cx.master().id_column()`, which `build()` guarantees |
| `Augmentation::augment_row(.., &VistaCatalog)` | `augment_row(dio_name, id_column, row)`, no resolver |
| `Diorama`, `composition`, `master_aggregate`, `HotTier`, `dio::refresh`, `DioError` | removed; none had a working use |
| `LensBuilder: Default` | `Lens::builder()` is the one entry point |
| `LensCallbacks`, `SceneryChunkTarget`, `ChunkWrite`, `ChunkRow`, `FlushReport`, the `Dio*Callback` aliases | crate-private |

## Behaviour notes

- **A paged loader is never called with a query it can't answer.** A paged view whose query
  leaves a clause the master doesn't take (a refused condition, search or order, one on an
  augmented column, the Dio's residual) is served from one whole copy made through the loader's
  `or_eager` start; without one the view fails. A grid over a REST source without `Order` or
  `Search` needs `Loader::paged(..).or_eager(..)`, and a `Refresher` to keep that copy fresh. The
  `or_eager` start runs only for a master without `Window` (it used to run for every Dio).
- **`refresh()` re-reads the way the set was loaded**: a paged Dio announces `DatasetChanged`, an
  eager one re-runs the start, any other reconciles with `master.list()`. An augmented Dio runs
  its augmentation pass and then the refresher. There is no timer without `Refresher::every`, and
  `max_age` without `every` checks staleness at build and at open.
- **A live follow holds each pushed row against the Dio's query**: a row that left the set applies
  as a delete, and over native conditions every change re-reads the set. A follow that lags or
  loses its subscription re-reads, retrying with backoff; a failed re-read is
  `DioEvent::LoadFailed { range: 0..0, .. }`, and a failed refresh is `DioEvent::RefreshFailed`.
- **`get_ref` on a row without its join column is an error**; a `get_ref` target inherits `live`
  but not augmentation.
- **A clause on a dotted path under an augmented column** (`detail.size`) stays in the residual,
  where 1.x offered it to the master. A Dio derived with an order its master refused keeps it as
  the residual's order: `fetch` names it in `refused`, and facade reads sort by it.
- **The facade advertises `Search` and `FilterOps`** and evaluates what the master doesn't take.
  Every facade read keeps only rows the residual admits; an order on its own reads the cache and
  sorts it, and only a condition or search the master takes sends the read to the master.
  `preview().note` names what is applied over the cache.

## Checklist

- [ ] every Lens is `Lens::builder()….build()?`, with no `Arc::new` around it
- [ ] hooks are `Loader`, `Refresher`, `writer` and `Tuning`, each callback taking `LoaderCx`
- [ ] every `make_dio` call is `lens.dio(v).build().await?`; `.augment(..)` and `.live()` are on
      the builder
- [ ] no Dio is narrowed in place: `dio.derive()…build().await?`
- [ ] `dio.vista()` is `dio.cached()`; window reads are `dio.fetch(range, &query)` and check
      `refused`
- [ ] paged lenses over masters that refuse sort or search have an `or_eager` start
