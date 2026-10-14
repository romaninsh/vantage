# Sceneries

In 1.x two identical opens returned the same `Arc`, so one grid's `set_search` changed what another
grid showed, and an `.exclusive()` flag opted out. A scenery had two filter setters, a separate
shown range, a cap wrapper and its own sort enum. 2.0 gives each scenery one owner: every open is
its own view, and sharing moved underneath, to the fetched rows, so two views of one query still
make one request (decision 7: only identical queries share). The filter vocabulary is vista's
(`Condition`, `SortDirection`), so a grid's filter and a Dio's narrowing are the same type. Limit,
tail and search became builder options. Read [Scenery: Reactive Views](../intro/step7-scenery.md)
for the current shape.

The snippets open views on a Dio of `teams` (rows `a` red, `b` blue).

## One sort direction, one condition type

Diorama's `SortDir` and `OpCondition` are gone: use `vantage_vista::SortDirection` and
`vantage_vista::Condition`, both in vista's prelude.

```rust,compile_fail,E0432
use vantage_diorama::{OpCondition, SortDir};

let direction = SortDir::Asc;
```

```rust
use vantage_vista::prelude::*;

let direction = SortDirection::Ascending;
let not_blue = Condition::new("name", FilterOp::Ne, "blue");
# let _ = (direction, not_blue);
```

## The builder

`where_eq` and `where_op` are one `filter(Condition)`; `sort` is `order`; `initial_range` is gone
(the first viewport is `0..page_size`); `exclusive()` is gone because every view is exclusive.

```rust,compile_fail,E0599
# use vantage_diorama::{prelude::*, Cache, LoadState, Loader, LoaderCx, RecordStatus};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn row(n: &str) -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() }
# fn teams() -> Vista {
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
let view = dio
    .table_scenery()
    .where_eq("name", "red")
    .sort("name", SortDirection::Ascending)
    .exclusive()
    .open()
    .await?;
# Ok(())
# }
```

```rust
# use vantage_diorama::{prelude::*, Cache, LoadState, Loader, LoaderCx, RecordStatus};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn row(n: &str) -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() }
# fn teams() -> Vista {
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
let view = dio
    .table_scenery()
    .filter(Condition::new("name", FilterOp::Eq, "red"))
    .order("name", SortDirection::Ascending)
    .page_size(50)
    .open()
    .await?;
let latest = dio.table_scenery().order("name", SortDirection::Ascending).tail(10).open().await?;
# let _ = (view, latest);
# Ok(())
# }
```

`CappedScenery::wrap(s, n)` and ui-diorama's `open_tail(n)` are `.limit(n)` and `.tail(n)` on the
builder. `tail` needs an order, and the two together are refused.

## The setters

`set_sort(col, dir)` is `set_order(Option<(col, dir)>)`, with `None` clearing it. `set_filters`
(eq pairs) and `set_filter_terms` (operator terms) are one `set_filters(Vec<Condition>)`; an
order-insensitively equal set is a no-op. `set_shown_range` merges into the viewport:
`set_viewport(loaded, shown)`, where an empty `shown` means nothing is on screen.

```rust,compile_fail,E0599
# use vantage_diorama::{prelude::*, Cache, LoadState, Loader, LoaderCx, RecordStatus};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn row(n: &str) -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() }
# fn teams() -> Vista {
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
let view = dio.table_scenery().open().await?;
view.set_sort(Some("name".into()), SortDirection::Descending);
view.set_filter_terms(vec![Condition::new("name", FilterOp::Ne, "blue")]);
view.set_shown_range(Some(0..20));
if view.is_loading() { /* spinner */ }
# Ok(())
# }
```

```rust
# use vantage_diorama::{prelude::*, Cache, LoadState, Loader, LoaderCx, RecordStatus};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn row(n: &str) -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() }
# fn teams() -> Vista {
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
let view = dio.table_scenery().open().await?;
view.set_order(Some(("name".into(), SortDirection::Descending)));
view.set_filters(vec![Condition::new("name", FilterOp::Ne, "blue")]);
view.set_viewport(0..50, 0..20);
match view.load_state() {
    LoadState::Loading => { /* spinner */ }
    LoadState::Failed(failure) => eprintln!("{failure}"), // shown where the grid shows "empty"
    _ => {}
}
# Ok(())
# }
```

`LoadState::Failed(LoadFailure)` is new, and `LoadState` is no longer `Copy`. A view fails, shows
no rows and emits `DioEvent::LoadFailed` when the master rejects its query, or when it is a paged
view whose query the master can't answer and the loader has no `or_eager` start
([Dio and Lens](./dio-lens.md#behaviour-notes)). `LoadFailure.clause` names the `Clause`:
`Filter(column)`, `Order(column)` or `Search`.

## Record sceneries open through a builder

`record_scenery(id).await` becomes `record_scenery(id).open().await`, and the `_sugared` and
`_with` variants become builder options. `load_on_miss()` is new: a cache miss reads the master,
and caches the row only when the Dio admits it.

```rust,compile_fail,E0599
# use vantage_diorama::{prelude::*, Cache, LoadState, Loader, LoaderCx, RecordStatus};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn row(n: &str) -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() }
# fn teams() -> Vista {
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
let record = dio.record_scenery_with("a", row("red")).await?;
let ready = matches!(record.status(), RecordStatus::Fresh | RecordStatus::Stale);
# Ok(())
# }
```

```rust
# use vantage_diorama::{prelude::*, Cache, LoadState, Loader, LoaderCx, RecordStatus};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn row(n: &str) -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() }
# fn teams() -> Vista {
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
let record = dio.record_scenery("a").seed(row("red")).open().await?;
let ready = match record.status() {
    RecordStatus::Fresh => true,
    RecordStatus::Loading | RecordStatus::Incomplete => false,
    _ => false,
};
# let _ = ready;
# Ok(())
# }
```

`RecordStatus::Stale`, `RowStatus::{Stale, Loading, NotFound}`, `ValueStatus::Stale` and
`EnrichedRecord::dirty_fields` are removed; `RecordStatus::Loading` and `Incomplete` are now
produced. The status enums are `#[non_exhaustive]`, so a match ends in `_ =>`.

## Renames

| 1.x | 2.0 |
|---|---|
| `request_refresh()` on table, record and value sceneries | `refresh_source()` |
| `filter_terms()` | `view_stats().filters` (the Dio's residual first, then the view's) |
| `terms_supported(&[OpCondition])`, `count_where(Vec<OpCondition>)` | `filters_supported(&[Condition])`, `count_where(Vec<Condition>)` |
| `record_scenery_sugared(id, s)` | `record_scenery(id).sugar(s).open()` |
| `SceneryDiagnostic::refcount` | `shared_by`, one per open view |
| `ViewStats.order: Option<(String, SortDir)>` | `Option<(String, SortDirection)>`, the view's declared order |
| `TableScenery::demanded_columns` | removed from the trait |
| default bodies on `TableScenery` methods | every method required; `row_by_id(id)` is new |

## Behaviour notes

- **Owned views.** Two opens are two handles; `Arc::ptr_eq` on them is false. With
  `diagnostics`, `dio.diagnostics().await?.sceneries[i].shared_by` counts the views sharing one
  set of rows.
- **A scenery on a derived Dio** layers the Dio's residual under its own clauses and, with no sort
  of its own, keeps the Dio's order.
- **Local refinement uses vista's evaluator**: cells are read with `cell_get`, `3 == 3.0`, null and
  absent cells sort last, ties break on id. Two-pass views and titles-only pickers now apply their
  conditions, search and the Dio's residual over the listed index; a picker whose query names an
  augmented column fails, since it never hydrates.
- **Augmented clauses hydrate first.** A refined two-pass view over a cold cache runs the detail
  pass for every listed row before it settles, and reads `Partial` while a row's detail fails.
- **A paged view's loader receives every clause** (filters, search, order) in its `VistaQuery`;
  `set_filters` on a paged view refetches the viewport. A total stated under a condition is not
  remembered as the table's total.
- **`search_supported()` is true for a paged view with an eager fallback**, so a UI shows its
  search box there.
- **Record, servo and value reads see only rows the Dio's residual admits**: a row outside a
  derived Dio's set is `NotFound` or absent.

## Checklist

- [ ] no `SortDir` or `OpCondition`; `SortDirection` and `Condition` from vantage-vista
- [ ] builders use `filter`, `order`, `limit`, `tail`, `search`; no `exclusive()` or `initial_range`
- [ ] `set_order(Option<..>)`, `set_filters(Vec<Condition>)`, `set_viewport(loaded, shown)`
- [ ] `is_loading()` is `load_state() == LoadState::Loading`, and `LoadState::Failed` is shown
- [ ] `record_scenery(id)…open().await?`; status matches end in `_ =>`
- [ ] `request_refresh` is `refresh_source`
