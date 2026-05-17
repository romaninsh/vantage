# Wiring Up a UI

This document is for someone connecting Diorama to a UI framework. The
running example is GPUI because that's what `vantage-ui` uses, but the
pattern generalizes to anything pull-on-render with an external notification
primitive (gpui, slint, cursive, egui).

The goal: a table widget that re-renders the visible rows when the underlying
data changes, a sheet widget that auto-updates when its record is mutated,
and a menu-bar counter that refreshes when the count changes. All three are
the same trick at different shapes.

## The three Scenery types

Every UI widget you wire to Diorama binds to one of three Sceneries.

- **`TableScenery`** for grids, lists, and any widget that displays a
  collection. Used by `gpui-component::DataTable`, `vantage-ui`'s
  `RecordGrid`, the perpetual log viewer, and anything similar.
- **`RecordScenery`** for forms, sheets, cards — anything that displays a
  single record. Used by the right-aligned detail sheet in `vantage-ui`.
- **`ValueScenery`** for badges, counters, single-value displays. Used by
  menu-bar unread counts, summary numbers in dashboards, any place a single
  aggregated value needs to refresh.

You spawn each from a Dio:

```rust
let table   = dio.table_scenery().sort("price", Asc).open();
let sheet   = dio.record_scenery(product_id);
let counter = dio.value_scenery().aggregate(Aggregate::count_where("unread", true)).open();
```

Each Scenery is `Arc<dyn TableScenery>` (or `RecordScenery` / `ValueScenery`).
Cheap to clone, cheap to drop. Dropping the last clone tears down the
Scenery's background fetcher.

## The data-flow contract UIs actually want

GPUI's `TableDelegate` polls the delegate on every render frame:

```rust
fn rows_count(&self, cx: &App) -> usize;
fn render_td(&self, row_ix, col_ix, window, cx) -> impl IntoElement;
```

This means the binding has to satisfy two things:

1. **Reads must be cheap and synchronous.** `scenery.row(idx)` is called for
   every visible cell on every render. It must return from in-memory state
   without awaiting anything. The Scenery's internal hot tier handles this
   — rows are materialized into `Arc<EnrichedRecord>` and held in memory for
   as long as the viewport touches them.
2. **Updates push, but UI pulls.** When data changes, the Scenery bumps a
   generation counter and signals on its watch channel. The UI adapter
   bridges that signal to `cx.notify()`, which triggers GPUI to re-render,
   which re-polls the Scenery. The Scenery never directly tells the UI
   "render this cell now."

This is why the dialogue's `mpsc<Delta>` Patch/Insert/Remove model would have
been a wrong fit — GPUI doesn't want deltas, it wants "something changed,
re-poll yourself."

## The GPUI binding pattern

For each Scenery type, the bridge is the same shape. A `SceneryEntity` wraps
the Scenery, owns a task that listens for changes, and calls `cx.notify()`.

```rust
use std::sync::Arc;
use gpui::{Context, Entity, Task};
use vantage_diorama::scenery::TableScenery;

pub struct TableSceneryEntity {
    scenery: Arc<dyn TableScenery>,
    _watch_task: Task<()>,
}

impl TableSceneryEntity {
    pub fn new(scenery: Arc<dyn TableScenery>, cx: &mut Context<Self>) -> Self {
        let mut rx = scenery.subscribe();
        let task = cx.spawn(|this, mut cx| async move {
            while rx.changed().await.is_ok() {
                this.update(&mut cx, |_, cx| cx.notify()).ok();
            }
        });
        Self { scenery, _watch_task: task }
    }

    pub fn scenery(&self) -> &Arc<dyn TableScenery> {
        &self.scenery
    }
}
```

The `_watch_task` is held to keep the listener alive. When the entity is
dropped, the task is cancelled, the watch receiver drops, and the Scenery's
notification fanout sees one fewer subscriber. Clean teardown.

`RecordSceneryEntity` and `ValueSceneryEntity` are the same pattern with
different inner trait types. You can write one generic version if your
GPUI/Rust setup allows it; copy-pasting three small structs is also fine.

### Why is the bridge necessary at all?

GPUI doesn't natively know about Tokio watch channels. The bridge task
translates the watch primitive into GPUI's native notification mechanism
(`cx.notify()`). Different UI frameworks will use different notification
primitives — Slint signals, cursive's event loop, egui's repaint requests —
but the pattern is identical: spawn a task, await the watch, call the
framework's "I changed" function.

## Implementing TableDelegate against TableScenery

Once you have the entity, the `TableDelegate` impl is mechanical. Every
method delegates to the Scenery:

```rust
use gpui_component::table::{TableDelegate, SortDirection};

impl TableDelegate for ProductsTable {
    fn rows_count(&self, _cx: &App) -> usize {
        self.scenery.row_count()
    }

    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }

    fn column(&self, ix: usize, _cx: &App) -> &Column {
        &self.columns[ix]
    }

    fn render_td(&self, row_ix: usize, col_ix: usize, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let column = &self.columns[col_ix];
        match self.scenery.row(row_ix) {
            Some(record) => render_cell(record, column),
            None => skeleton_cell(column),                    // not yet loaded
        }
    }

    fn has_more(&self) -> bool {
        self.scenery.has_more()
    }

    fn load_more(&mut self, _w: &mut Window, _cx: &mut Context<Self>) {
        self.scenery.request_load_more();
    }

    fn visible_rows_changed(&mut self, range: Range<usize>, _w: &mut Window, _cx: &mut Context<Self>) {
        self.scenery.set_viewport(range);                     // drives prefetch
    }

    fn perform_sort(&mut self, col_ix: usize, sort: SortDirection, _w: &mut Window, _cx: &mut Context<Self>) {
        let column = &self.columns[col_ix];
        self.scenery.set_sort(Some(column.field.clone()), sort.into());
    }
}
```

The `skeleton_cell` is the placeholder for rows the Scenery doesn't have yet
— a shimmer, a "Loading…" cell, whatever fits your design. As the Scenery's
background fetcher loads more rows and bumps generation, GPUI re-renders and
those cells turn into real data.

### What about quicksearch?

Bind your search input to `scenery.set_search(Some(query))`. The Scenery's
fetcher reacts: clears the loaded rows, fetches matching results (from
master if the master supports search, from the cache if not), updates row
count, bumps generation. The grid re-renders showing the new result set.

```rust
input.on_change(cx, |this, ev, cx| {
    let text = ev.text.clone();
    this.scenery.set_search((!text.is_empty()).then(|| text));
    cx.notify();
});
```

### Column sort

Same idea. `scenery.set_sort(column, direction)` triggers the Scenery to
re-fetch in the new order. The Scenery decides whether to push the sort down
to master (if `master.can_order()` is true) or to sort the cache locally.

## RecordScenery — the detail sheet

The current pattern in `vantage-ui` is "double-click row → open sheet with
record fetched once." RecordScenery upgrades this to "sheet that
auto-refreshes when the underlying record changes" — driven by the same
event bus that updates the grid.

```rust
pub struct RecordSheetView {
    entity: Entity<RecordSceneryEntity>,
    columns: Vec<Column>,
}

impl Render for RecordSheetView {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scenery = self.entity.read(cx).scenery();
        let record = match scenery.record() {
            Some(r) => r,
            None => return Self::loading_view(),
        };
        let dirty = record.dirty_fields.as_deref().unwrap_or(&[]);

        v_flex().children(self.columns.iter().map(|col| {
            field_row(col, &record, dirty.contains(&col.field))
        }))
    }
}
```

Opening the sheet:

```rust
fn on_row_double_clicked(&mut self, row: &Record, cx: &mut Context<Self>) {
    let scenery = self.dio.record_scenery(row.id());
    let entity = cx.new_entity(|cx| RecordSceneryEntity::new(scenery, cx));
    self.window.open_sheet(RecordSheetView { entity, columns: self.columns.clone() });
}
```

When some other code (a different sheet, an external event, a background
refresh) updates the same record, the Dio's event bus publishes
`RecordChanged { id }`, the RecordScenery sees its id mentioned, bumps
generation, and the sheet re-renders. The user sees the form values update
in place.

### What if the record is deleted while the sheet is open?

`scenery.status() == RecordStatus::NotFound`. The sheet's render method
checks status and shows a "this record was deleted" message, or auto-closes,
or whatever the design calls for. Status is the universal "something
non-data-shaped is going on" channel — also used for `Loading`,
`PendingWrite`, `Stale`.

## ValueScenery — counters and badges

The smallest Scenery type. Returns a single `CborValue` plus a status. Used
for everything that's "one number, refreshes":

```rust
let unread_count = dio.value_scenery()
    .aggregate(Aggregate::count_where("read", false))
    .open();

let entity = cx.new_entity(|cx| ValueSceneryEntity::new(unread_count, cx));
```

Render is trivial — read the value, display it:

```rust
impl Render for UnreadBadge {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.entity.read(cx).scenery().value()
            .and_then(|v| v.as_integer())
            .unwrap_or(0);
        if count == 0 { return None; }
        Some(badge(count.to_string()))
    }
}
```

When any record under the underlying Dio changes, the ValueScenery
re-evaluates its aggregate (from the cache, cheaply) and bumps generation if
the result differs. The badge animates from "3" to "4" without you writing
any of that logic.

### Why CborValue?

ValueScenery supports counts (integer), sums (integer or float), maxes
(any comparable), and free-form aggregates that return a structured value.
`CborValue` covers all of them without needing one trait per type. Downcast
to what you expect at render time.

## Log viewer pattern

`vantage-ui`'s log viewer is currently push-based (`append`, `extend`,
`LinesAppended` events) and lives outside the Vista world. Two ways to
integrate:

**Option A (simple).** Keep the log viewer as-is. Don't model log lines as
Vista records. The widget is fine standing alone.

**Option B (unified).** Model log lines as a TableScenery over an append-only
Vista. The Scenery exposes `has_more() = true` perpetually; new lines arrive
via the Dio's event bus (`RecordInserted`). The log widget consumes the
generation bump and re-renders, picking up the new tail rows.

Option B is more uniform but adds a layer for a widget that doesn't need
random access. Pick A unless you want the same row-status semantics
(write-pending, failed) on log entries, in which case B earns its keep.

## Where the adapter code lives

UI-framework-specific glue lives in `vantage-ui-adapters`. That crate already
hosts the shared types used by multiple UI integrations — perpetual sources,
column metadata, row identity. Diorama's GPUI adapter lands there as a new
module:

```
vantage-ui-adapters/src/diorama/
├── mod.rs
├── table_entity.rs        // TableSceneryEntity
├── record_entity.rs       // RecordSceneryEntity
├── value_entity.rs        // ValueSceneryEntity
└── delegate.rs            // ProductsTable etc. example delegates
```

Cursive, slint, egui, tauri adapters live in sibling modules. Each is
20–80 lines: define the framework-native entity, bridge the watch channel,
delegate to the Scenery. No two adapters duplicate Scenery logic — they all
just call its methods.

## Pull-on-render performance

A few things to watch:

- **`row(idx)` allocates an `Arc::clone`.** Cheap, but if you have 10,000
  cells visible at once, that's 10,000 clones per render. GPUI virtualizes,
  so usually only ~50 rows are rendered. Verify with your grid's actual
  viewport size.
- **`set_viewport(range)` is called on every scroll.** The Scenery debounces
  internally — don't worry about flooding it. But your delegate impl should
  do the bare minimum of work in `visible_rows_changed`.
- **`request_load_more` should be idempotent.** GPUI may call it multiple
  times during a single scroll if the load-more threshold is crossed
  repeatedly. The Scenery deduplicates: a load-more request when one is
  already in flight is a no-op.

If your grid feels janky, the usual culprit is in your `render_td`, not in
the Scenery — heavy element construction per cell, expensive string
formatting, missed memoization opportunities. Profile before blaming the
data layer.

## A worked example end-to-end

Products grid with quicksearch, sort by column, double-click to open a
detail sheet that auto-refreshes.

```rust
use std::sync::Arc;
use gpui::{Context, Entity, Window};
use vantage_diorama::{Dio, scenery::TableScenery};

pub struct ProductsView {
    dio: Dio,
    grid: Entity<ProductsGrid>,
    search_input: Entity<TextInput>,
}

impl ProductsView {
    pub fn new(dio: Dio, cx: &mut Context<Self>) -> Self {
        let scenery: Arc<dyn TableScenery> = dio.table_scenery()
            .page_size(50)
            .sort("name", SortDir::Asc)
            .open();

        let entity = cx.new_entity(|cx| TableSceneryEntity::new(scenery, cx));
        let grid = cx.new_entity(|cx| ProductsGrid {
            entity,
            columns: product_columns(),
        });

        let dio_for_dbl = dio.clone();
        let window_for_dbl = cx.window_handle();
        cx.subscribe(&grid, move |_this, _grid, ev: &RowActivated, cx| {
            let scenery = dio_for_dbl.record_scenery(ev.id.clone());
            let sheet_entity = cx.new_entity(|cx| RecordSceneryEntity::new(scenery, cx));
            window_for_dbl.update(cx, |w, cx| {
                w.open_sheet(cx, |cx| ProductSheet { entity: sheet_entity });
            }).ok();
        });

        let search_input = cx.new_entity(|cx| {
            let mut input = TextInput::new(cx);
            let dio_for_search = dio.clone();
            input.on_change(cx, move |_, ev, _cx| {
                let q = ev.text.clone();
                dio_for_search.table_scenery_handle()
                    .set_search((!q.is_empty()).then(|| q));
            });
            input
        });

        Self { dio, grid, search_input }
    }
}
```

The grid renders. The search input filters live. Double-click opens an
auto-refreshing sheet. When any record under `dio` changes — locally, via
external event, via background refresh — the relevant Sceneries bump
generation, the bridges call `cx.notify()`, the UI updates. You wrote zero
synchronization code.
