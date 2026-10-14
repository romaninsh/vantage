# Surface

Most first-round compile errors in a port are paths. Rule 11 gives every public item one path: its
module or the crate root, never both, and no crate re-exports another crate's items outside its
prelude. Rule 8 made the internal modules private. The preludes shrank to what nearly every user
writes, each item under its home crate. Rule 7 put `#[non_exhaustive]` on the enums and data
structs that may grow, so matches gain a `_ =>` arm and structs are built through constructors.
Rule 9 moved test and debug helpers behind features. None of this changes behaviour; it changes
where things are found.

## The preludes

| Crate | Prelude |
|---|---|
| vantage-vista | `Vista`, `VistaQuery`, `Condition`, `FilterOp`, `SortDirection`, `Capability`, `VistaResolver`, `CborValue`, and the vantage-dataset value-set traits |
| vantage-diorama | `Lens`, `Dio`, `ChangeFlash`, `ChangeEvent`, `TableScenery`, `RecordScenery`, `ValueScenery`, `Servo`, `Aggregation` |

Everything else is imported from the crate root. Diorama's prelude no longer reaches `Cache`,
`Loader`, `LoaderCx`, `Refresher`, `Tuning`, `Augmentation`, `Detail`, `Fetch`, `MergeRule`,
`Source`, `DioEvent`, `Generation`, `WriteCapabilities`, `CacheBackend`, `CacheStatus`,
`CacheTable`, `RedbCache`, `FlashKind`, `FlashOutcome`, `EnrichedRecord`, `RecordStatus`,
`RowStatus`, `IdStrategy`, `ServoStatus` or vista's `VistaCatalog`. Vista's no longer reaches
`VistaFactory`, `TableShell` or `VistaCapabilities`.

```rust,compile_fail,E0433
use vantage_diorama::prelude::*;
# use vantage_vista::prelude::*;
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {

let lens = Lens::builder()
    .cache(Cache::memory())
    .loader(Loader::eager(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    }))
    .build()?;
# Ok(())
# }
```

```rust
use vantage_diorama::{prelude::*, Cache, Loader};
# use vantage_vista::prelude::*;
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {

let lens = Lens::builder()
    .cache(Cache::memory())
    .loader(Loader::eager(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    }))
    .build()?;
# let _ = lens;
# Ok(())
# }
```

A factory or shell author imports the traits from the root:

```rust,compile_fail,E0405
use vantage_vista::prelude::*;

fn register<F: VistaFactory>(_factory: F) {}
```

```rust
use vantage_vista::{prelude::*, VistaFactory};

fn register<F: VistaFactory>(_factory: F) {}
```

## Private modules

Diorama's `augment`, `dio`, `lens`, `ops`, `scenery` and `servo` modules are private; every public
item they held is at the crate root.

```rust,compile_fail,E0603
use vantage_diorama::scenery::{TableScenery, RecordStatus};
# use vantage_diorama::LoadState;

fn ready(view: &dyn TableScenery) -> bool {
    view.load_state() == LoadState::Complete
}
# fn main() { let _ = RecordStatus::Fresh; }
```

```rust
use vantage_diorama::{TableScenery, RecordStatus};
# use vantage_diorama::LoadState;

fn ready(view: &dyn TableScenery) -> bool {
    view.load_state() == LoadState::Complete
}
# fn main() { let _ = RecordStatus::Fresh; }
```

| 1.x | 2.0 |
|---|---|
| `vantage_diorama::scenery::{table, record, value, sugar, enriched_record}::X` | `vantage_diorama::X` |
| `vantage_diorama::lens::CacheTable`, `lens::Cache`, … | `vantage_diorama::CacheTable`, … |
| `vantage_diorama::dio::event_bus::DioEvent`, `ops::change_flash::ChangeFlash` | `vantage_diorama::DioEvent`, `vantage_diorama::ChangeFlash` |
| `vantage_diorama::servo::Servo`, `augment::Augmentation` | `vantage_diorama::Servo`, `vantage_diorama::Augmentation` |
| `vantage_diorama::{DebugTap, ProcessStats, process_stats}` | `vantage_diorama::debug::{…}` (the `diagnostics` feature) |
| `vantage_diorama::VistaCapabilities` | `vantage_vista::VistaCapabilities` |
| `vantage_diorama::debug::{dur, bytes, pct}` | crate-private; `debug::num` stays public |
| `DioShell`, `dio::shell` | crate-private; use the Vista from `dio.cached()` |
| `vantage_vista::{DataVocab, Handle, RecordDraft, run_script, …}` | `vantage_vista::rhai::…` |
| `vantage_memory::vista::{Catalog, MemoryTableShell, factory::…}` | `vantage_memory::{MemoryCatalog, MemoryTableShell, MemoryVistaFactory, …}` |
| `pub mod dio::{worker, event_bus, impls, shell, hot_tier, refresh}` | crate-private |

## Matches gain a wildcard arm

An exhaustive match on a diorama enum stops compiling, because a new variant is no longer a
breaking change. The enums this hits most: `FlashKind`, `ChangeEvent`, `DioEvent`, `LoadState`,
`Clause`, `RecordStatus`, `RowStatus`, `ValueStatus`, `ServoStatus`, `IdStrategy`, `CacheStatus`,
`Detail`, `Fetch`, `Source`, and vista's `Capability`, `FilterOp`, `ColumnFlag`, `ReferenceKind`,
`ReferenceSugar`, `Terminals` and `Writes`. `==` and `matches!` are unaffected.

```rust,compile_fail,E0004
# use vantage_diorama::{prelude::*, FlashKind};
fn label(flash: &ChangeFlash) -> &'static str {
    match flash.kind() {
        FlashKind::Insert => "insert",
        FlashKind::Patch => "patch",
        FlashKind::Replace => "replace",
        FlashKind::Upsert => "upsert",
        FlashKind::Delete => "delete",
        FlashKind::Clear => "clear",
    }
}
# fn main() { let _ = label(&ChangeFlash::clear()); }
```

```rust
# use vantage_diorama::{prelude::*, FlashKind};
fn label(flash: &ChangeFlash) -> &'static str {
    match flash.kind() {
        FlashKind::Insert => "insert",
        FlashKind::Patch => "patch",
        FlashKind::Replace => "replace",
        FlashKind::Upsert => "upsert",
        FlashKind::Delete => "delete",
        FlashKind::Clear => "clear",
        _ => "write",
    }
}
# fn main() { assert_eq!(label(&ChangeFlash::clear()), "clear"); }
```

## Structs are built through constructors

A struct literal of a `#[non_exhaustive]` struct from another crate doesn't compile. Reading its
public fields still works.

```rust,compile_fail,E0639
# use std::sync::Arc;
# use vantage_diorama::{Augmentation, Detail, Fetch, MergeRule, Source};
# use vantage_vista::{mocks::MockShell, Vista};
# fn main() {
# let sizes = Arc::new(Vista::new("sizes", MockShell::new()));
let size = Augmentation {
    detail: Detail::Fixed(sizes),
    source: Source::Id,
    fetch: Fetch::PerRow,
    merge: MergeRule { columns: vec!["size".to_string()] },
};
# }
```

```rust
# use std::sync::Arc;
# use vantage_diorama::{Augmentation, Detail, Fetch, MergeRule, Source};
# use vantage_vista::{mocks::MockShell, Vista};
# fn main() {
# let sizes = Arc::new(Vista::new("sizes", MockShell::new()));
let size = Augmentation::new(Detail::Fixed(sizes), Source::Id, Fetch::PerRow, MergeRule::lift(["size"]));
# assert_eq!(size.merge.columns, ["size"]);
# }
```

| Struct | Build it with |
|---|---|
| `Augmentation`, `MergeRule`, `AggregationSpec` | `Augmentation::new(..)`, `MergeRule::lift(cols)` / `lift_all()`, `AggregationSpec::new(..)` |
| `EnrichedRecord` | `EnrichedRecord::fresh(r)`, `incomplete`, `pending_write`, `write_failed`, `detail_failed` |
| `Tuning` | `Tuning::default().with_*(..)` |
| `VistaSpec`, `ColumnSpec`, `ReferenceSpec`, `ContainedYaml`, `NoExtras` | `VistaSpec::new(name)`, `ColumnSpec::new()`, deserialise, `NoExtras::default()` |
| `ImportOutcome`, `DioDiagnostics`, `SceneryDiagnostic`, `Fetched`, `FlashOutcome`, `ViewStats` | read their fields; `FlashOutcome::applied()` |

## Traits you implement

The open extension traits lost their default bodies (rule 2), so an implementation states every
method:

- **`TableShell`**: `metadata()`, `Clone`, and `unsupported(..)` for what it doesn't do; see
  [Vista](./vista.md#for-shell-authors).
- **`CacheBackend` and `CacheTable`**: `close`, `list_tables`, `drop_table` and `name` on the
  backend; `meta_total`, `meta_refreshed_at` and `meta_copied_at` with their setters, and the
  `*_with_status` reads and writes, on the table. A wrapper forwards each one, status included, and
  `clear` forgets the refresh and copy stamps. A backend that can't enumerate or drop tables
  returns an error.
- **`TableScenery`**: every method, including `row_by_id`; `demanded_columns` left the trait.
- **`Aggregation` and `GroupReducer`**: `reads()`, and the schema parameter
  ([Aggregates](./aggregates.md#a-custom-aggregation)).

## Features

| Feature | Gates |
|---|---|
| vantage-diorama `diagnostics` | `debug` (`DebugTap`, `num`, `ProcessStats`, `process_stats`), `stats`, `Dio::debug_tap`, `LensBuilder::debug_datasource`, `Dio::diagnostics`, `DioDiagnostics`, `SceneryDiagnostic`; a default build records no fetch ledger |
| vantage-diorama `test-util` | `Dio::take_write_worker_handle` |
| vantage-vista `mocks` | `vantage_vista::mocks` (`MockShell`, `MockCalls`); it enables tokio's `sync` |
| vantage-table `mocks` | `vantage_table::mocks`, `prelude::MockTableSource` |

`Dio::live_table_scenery_count()` is `dio.diagnostics().await?.sceneries.len()`, and
`diagnostics()` returns a `Result`: a cache that can't count its rows is an error naming the
table, where it read as `cache_rows: 0`.

## Checklist

- [ ] imports outside the preludes come from the crate root
- [ ] no `vantage_diorama::{scenery, lens, dio, ops, servo, augment}::` paths
- [ ] every match on a vista or diorama enum ends in `_ =>`
- [ ] no struct literal of a `#[non_exhaustive]` struct
- [ ] `diagnostics`, `test-util` and `mocks` are enabled where their items are used
