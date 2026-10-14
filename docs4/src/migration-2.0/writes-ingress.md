# Writes and ingress

1.x wrote through a family of `flash_*` methods, a facade that returned before the write landed
(reporting a failure only as an event), and an id-less insert that skipped the Lens writer. Changes
from outside came in through `patched`, `notify_record_changed`, `reconcile_values` and
`handle_event`. Rule 1 cut both sides to one call each. Outbound, every write is a `ChangeFlash`
handed to `Dio::flash`, which joins the Dio's write queue and returns once the write is confirmed
or rolled back (decision 5: the outbound half of the photography lexicon). Inbound, one change is
`apply_change(ChangeEvent)` and a full set is `reconcile(snapshot)`. Read
[A Facade over a Custom API](../intro/step5-dio-lens.md#writes-on-a-read-only-master) and
[Live Data](../live-data.md#changes-you-feed-in-yourself) for the current flow.

The snippets run over a Dio of `teams` (rows `a` red, `b` blue) whose eager loader copies the
master; `row(name)` builds a one-column record.

## One write call

`flash_insert`, `flash_patch`, `flash_replace`, `flash_delete` and `flash_returning_id` become
`dio.flash(ChangeFlash::…)`, which returns a `FlashOutcome`. `ChangeFlash::new` is private; each
kind has its constructor (`insert`, `insert_new`, `partial`, `replace`, `upsert`, `delete`,
`clear`), so a kind always comes with an id the queue can act on.

```rust,compile_fail,E0599
# use vantage_diorama::{prelude::*, Cache, DioEvent, FlashOutcome, Loader, LoaderCx};
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
dio.flash_patch("a", row("green")).await?;
let id = dio.insert_returning_id(row("white")).await?;
# Ok(())
# }
```

```rust
# use vantage_diorama::{prelude::*, Cache, DioEvent, FlashOutcome, Loader, LoaderCx};
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
dio.flash(ChangeFlash::partial("a", row("green"))).await?;
let id = dio.flash(ChangeFlash::insert_new(row("white"))).await?.id;
# let a = dio.cached().get_value("a").await?.and_then(|r| r.get("name").cloned());
# assert_eq!(a, Some(CborValue::Text("green".into())));
# assert!(id.is_some());
# Ok(())
# }
```

`insert_new` replaces the crate-private `insert_returning_id`: the id-less insert now goes through
the queue and through the Lens writer when there is one, and `FlashOutcome::id` is the id the row
settled under.

## The writer returns an outcome

`on_flash` becomes `.writer(f)`. The callback takes `(LoaderCx, ChangeFlash)` and returns
`Result<FlashOutcome>`: `FlashOutcome::applied()` confirms the write, `.with_id(id)` reports the id
an id-less insert got (and re-keys the staged row onto it), `.with_record(r)` hands back the stored
record.

```rust,compile_fail,E0599
# use vantage_diorama::{prelude::*, Cache, DioEvent, FlashOutcome, Loader, LoaderCx};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn row(n: &str) -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() }
# fn teams() -> Vista {
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let lens = Lens::builder()
    .cache(Cache::memory())
    .on_flash(|flash: ChangeFlash| async move {
        println!("journal: {:?}", flash.kind());
        Ok(())
    })
    .build()?;
# Ok(())
# }
```

```rust
# use vantage_diorama::{prelude::*, Cache, DioEvent, FlashOutcome, Loader, LoaderCx};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn row(n: &str) -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() }
# fn teams() -> Vista {
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let lens = Lens::builder()
    .cache(Cache::memory())
    .writer(|_cx: LoaderCx, flash: ChangeFlash| async move {
        println!("journal: {:?}", flash.kind());
        let outcome = FlashOutcome::applied();
        Ok(match flash.id() {
            Some(_) => outcome,
            None => outcome.with_id("j-1"), // an id-less insert: report the id it got
        })
    })
    .build()?;
let dio = lens.dio(teams()).build().await?;
let outcome = dio.flash(ChangeFlash::insert_new(row("green"))).await?;
# let _ = outcome;
# Ok(())
# }
```

Writes on one Dio run one at a time, in order, and a writer must not flash the Dio it serves: that
returns an error naming the Dio, where 1.x ran it inline. A writer that relied on concurrent calls
now sees them one at a time per Dio. A writer that panics fails its flash, which is rolled back,
and the queue carries on. A caller that stops waiting no longer cancels its write, and a rollback
of stacked flashes restores the last confirmed row.

## Changes from outside

`patched`, `removed` and `notify_record_changed` become `apply_change(ChangeEvent)`;
`reconcile_value`, `reconcile_values` and `notify_dataset_changed` become `reconcile(snapshot)`,
which makes the cache hold exactly that set; `handle_event` (which only dispatched to `on_event`)
is gone.

```rust,compile_fail,E0599
# use vantage_diorama::{prelude::*, Cache, DioEvent, FlashOutcome, Loader, LoaderCx};
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
dio.patched("a", row("green")).await?;
dio.reconcile_values(dio.master().list_values().await?).await?;
# Ok(())
# }
```

```rust
# use vantage_diorama::{prelude::*, Cache, DioEvent, FlashOutcome, Loader, LoaderCx};
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
dio.apply_change(ChangeEvent::Updated { id: "a".into(), new: Some(row("green")) }).await?;
dio.reconcile(dio.master().list_values().await?).await?;
# Ok(())
# }
```

`Updated { id, new: None }` is the notify-only shape (`notify_record_changed`); `Deleted { id }`
is `removed(id)`; `Invalidated` re-reads the set. `reconcile` skips rows with a write in flight,
deletes cached rows absent from the snapshot, stamps the refresh time and publishes one
`DatasetChanged`. Inside a loader or refresher, `cx.reconcile(rows)` is the same call.

## Facade writes are awaited

`dio.cached()` inserts, replaces, upserts, patches and deletes no longer return before the write
lands. Each one awaits `Dio::flash` and returns once the write is confirmed, or with its error once
the staged row is rolled back. Polling loops that waited for a facade write to show up can go,
and the returned error has to be handled where it used to be ignored. The record a facade write
hands back carries the id under the Dio's id column, not a literal `"id"`.

```rust
# use vantage_diorama::{prelude::*, Cache, DioEvent, FlashOutcome, Loader, LoaderCx};
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
let mut events = dio.subscribe_events();
dio.cached().insert_value("c", &row("green")).await?; // confirmed when this returns
dio.flash(ChangeFlash::delete("b")).await?;

while let Ok(event) = events.try_recv() {
    match event {
        DioEvent::RecordInserted { id, .. } => println!("+ {id}"),
        DioEvent::RecordRemoved { id, .. } => println!("- {id}"),
        DioEvent::RecordChanged { id, .. } => println!("~ {id}"),
        _ => {}
    }
}
# Ok(())
# }
```

A confirmed write publishes its settle event by membership: a delete is `RecordRemoved`, a write
of a row the cache didn't hold is `RecordInserted`, any other write `RecordChanged`. 1.x published
`RecordChanged` for all three, so a subscriber that reacted to it after an insert or a delete
handles the other two as well. A re-keyed row publishes `RecordRemoved` for the staged id and
`RecordInserted` for the settled one. `DioEvent::WriteFailed` now announces only a failed write
that staged no row (an id-less insert, `Clear`).

## Writes are confined to the Dio's set

A derived Dio's writes are held to its full query. Diorama checks the residual itself: a record
gets its `Eq` values filled in, and a record or patch result outside the set, or a write to an id
held outside it, is `Conflict`. A patch of a row the set doesn't hold is `NotFound`, and a delete
of one writes nothing. Under a Lens writer the whole query is checked, and a `Clear` (facade
`delete_all`) becomes one `Delete` per row in the set. A native `import_values` is refused as
`Conflict` when one record falls outside the residual. A native condition can't be judged in
memory, so under a writer a Dio narrowed by one refuses every write, naming the clause. A write
that relied on an `Eq` on an augmented column being filled into the record supplies the column
itself. A set narrowed by a dotted path (`address.city = x`) records no invariant: a full-record
write must hold `x` at `address.city` itself, and a patch must leave the row in the set, or it is
`Conflict`. vantage-memory follows the same rule.

## Renames

| 1.x | 2.0 |
|---|---|
| `flash_insert(id, r)`, `flash_patch(id, f)`, `flash_replace`, `flash_delete(id)` | `flash(ChangeFlash::insert(id, r))`, `::partial(id, f)`, `::replace`, `::delete(id)` |
| `flash_returning_id`, `insert_returning_id` | `flash(ChangeFlash::insert_new(r)).await?.id` |
| `ChangeFlash::new(FlashKind::Patch, Some(id), f)` | `ChangeFlash::partial(id, f)`, one constructor per kind |
| writer returning `Result<()>` | `Result<FlashOutcome>`: `FlashOutcome::applied()` |
| `patched(id, r)`, `removed(id)`, `notify_record_changed(id)` | `apply_change(ChangeEvent::Updated { id, new: Some(r) })`, `Deleted { id }`, `Updated { id, new: None }` |
| `reconcile_value(s)`, `notify_dataset_changed`, `notify_seeded` | `reconcile(snapshot)` or `refresh()`; the Dio announces a detached eager start itself |
| rhai `record.save()`, `servo.save()` | `flash()` |

## Checklist

- [ ] every write is `dio.flash(ChangeFlash::…)`; no `flash_*` method or `ChangeFlash::new`
- [ ] the Lens writer returns `FlashOutcome`, with `.with_id(..)` for id-less inserts
- [ ] facade write callers handle the returned error and drop their polling
- [ ] `subscribe_events` consumers handle `RecordInserted` and `RecordRemoved`, and end their match
      with `_ =>`
- [ ] external changes go through `apply_change` or `reconcile`
- [ ] writers over narrowed Dios expect one `Delete` per row in place of `Clear`
