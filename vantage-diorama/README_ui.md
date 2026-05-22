# Wiring Up a UI

This document is for someone connecting Diorama to a UI framework. The
running example is GPUI (specifically `gpui-component::Table`) because that's
what `vantage-ui` uses, but the pattern generalizes to anything pull-on-render
with an external notification primitive — slint, cursive, egui, tauri.

The goal: a table widget that shows the rows currently sitting in the local
cache, re-renders when the cache changes, and lets the user scroll, search,
and sort that cache as a live viewport. Plus a sheet that auto-updates when
its record is mutated, and a counter that refreshes when the underlying
aggregate changes.

## What Diorama gives a UI

Diorama is the layer that turns "rows on disk in a redb file" into something a
GPUI widget can render. The cache is the source of truth on the UI side:

- Reads served from cache → fast and synchronous from the render-frame's POV.
- Writes go through the Dio's write queue → optimistic-by-default if your
  `on_write` callback applies to the cache first.
- External changes arrive via the event bus → Sceneries notice and bump a
  generation; widgets re-render.

UIs talking to Diorama answer one question: *what is in our cache right now?*
Not *what could be fetched from master* — that's the Lens's `on_refresh` /
`on_event` problem.

## The three Scenery types

Every UI widget you wire to Diorama binds to one of three Sceneries. They
all live in [`vantage_diorama::scenery`](src/scenery/mod.rs).

- **`TableScenery`** — a grid or list. Spawned via `dio.table_scenery()…
  .open().await?`, returns `Arc<dyn TableScenery>`.
- **`RecordScenery`** — a single row by id. Spawned via
  `dio.record_scenery(id).await?` or `dio.record_scenery_with(id, record)` if
  you already hold the row.
- **`ValueScenery`** — a single scalar (count, sum, custom aggregate). Spawned
  via `dio.value_scenery()…open().await?`.

Each handle is `Arc<dyn …>`. Cheap to clone, cheap to drop. The internal
reload task watches the Dio's event bus and bumps a watch channel; the last
external handle dropping lets the task wind down (the Dio is held weakly).

```rust
use vantage_diorama::scenery::{Aggregate, SortDir, TableScenery};

let table = dio.table_scenery()
    .sort("price", SortDir::Asc)
    .open()
    .await?;

let sheet = dio.record_scenery("sku-1234".to_string()).await?;

let counter = dio.value_scenery()
    .aggregate(Aggregate::CountWhere(vec![
        ("unread".to_string(), true.into()),
    ]))
    .open()
    .await?;
```

## The pull-on-render contract

GPUI's `TableDelegate` polls the delegate on every render frame. The relevant
shape (see `gpui-component/crates/ui/src/table/delegate.rs`):

```rust
fn rows_count(&self, cx: &App) -> usize;
fn render_td(
    &mut self,
    row_ix: usize,
    col_ix: usize,
    window: &mut Window,
    cx: &mut Context<TableState<Self>>,
) -> impl IntoElement;
fn has_more(&self, cx: &App) -> bool;                  // default: false
fn load_more_threshold(&self) -> usize;                // default: 20
fn load_more(&mut self, _: &mut Window, _: &mut Context<TableState<Self>>);
fn visible_rows_changed(&mut self, range: Range<usize>, ...);
```

Two implications:

1. **Reads are cheap and synchronous.** `scenery.row(idx)` returns from
   in-memory `Vec<Arc<EnrichedRecord>>` without awaiting. The Scenery loads
   matching cached rows up front, so the row vector is populated by the time
   `.open().await` returns.
2. **Updates push, but UI pulls.** When the cache changes, the Scenery bumps
   a generation on its watch channel. The UI adapter bridges that to
   `cx.notify()`, which triggers GPUI to re-render, which re-polls the
   delegate. The Scenery never directly tells the UI "render this cell now."

## The GPUI binding pattern

For each Scenery type, the bridge is the same shape. A `SceneryEntity` wraps
the handle, owns a task that listens for changes, and calls `cx.notify()`.

```rust
use std::sync::Arc;
use gpui::{Context, Task};
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
dropped, the task is cancelled, the watch receiver drops, the Scenery's
notification fanout sees one fewer subscriber, and (once nothing else holds
the `Arc<dyn TableScenery>`) its internal reload task tears down on the next
event cycle.

`RecordSceneryEntity` and `ValueSceneryEntity` are the same pattern with
different inner trait types.

## Implementing `TableDelegate` against `TableScenery`

```rust
use std::ops::Range;
use gpui::{App, Context, Window, IntoElement};
use gpui_component::table::{Column, ColumnSort, TableDelegate, TableState};
use vantage_diorama::scenery::SortDir;

pub struct ProductsTable {
    entity: gpui::Entity<TableSceneryEntity>,
    columns: Vec<ColumnSpec>,
}

impl TableDelegate for ProductsTable {
    fn rows_count(&self, cx: &App) -> usize {
        self.entity.read(cx).scenery().row_count()
    }

    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }

    fn column(&self, ix: usize, _cx: &App) -> Column {
        self.columns[ix].to_column()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _w: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let column = &self.columns[col_ix];
        // `cx.entity()` here is the TableState; reach the Scenery through the
        // bridge entity we stashed in `self`.
        let scenery = self.entity.read(cx.entity().read(cx).app()).scenery().clone();
        match scenery.row(row_ix) {
            Some(record) => render_cell(&record, column),
            None => skeleton_cell(column),       // rare — only between rebuilds
        }
    }

    fn has_more(&self, cx: &App) -> bool {
        self.entity.read(cx).scenery().has_more()
    }

    fn load_more(&mut self, _: &mut Window, cx: &mut Context<TableState<Self>>) {
        // Random-access masters (SQLite/Postgres/CSV/REST with offset)
        // are driven entirely by `set_viewport` from
        // `visible_rows_changed` — jumping the scrollbar already loads
        // the visible band. Cursor-only masters (DynamoDB, token-paged
        // REST) can't be queried by index, so `request_load_more`
        // walks the cache forward one page at a time.
        let scenery = self.entity.read(cx).scenery();
        if scenery.master_capabilities().can_fetch_page {
            return;
        }
        scenery.request_load_more();
    }

    fn visible_rows_changed(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        // Forward the visible band straight to the Scenery; the
        // viewport pipeline debounces, deduplicates, and dispatches
        // `on_load_chunk` for any uncached ranges.
        self.entity.read(cx).scenery().set_viewport(range);
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _w: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let column = &self.columns[col_ix];
        let dir = match sort {
            ColumnSort::Ascending => SortDir::Asc,
            ColumnSort::Descending => SortDir::Desc,
            ColumnSort::Default => SortDir::Asc,
        };
        self.entity
            .read(cx.entity().read(cx).app())
            .scenery()
            .set_sort(Some(column.field.clone()), dir);
    }
}
```

`render_cell` and `skeleton_cell` are your business. `EnrichedRecord` exposes
`record: Record<CborValue>` plus `status: RowStatus` plus a `dirty_fields`
slot (reserved for form editing in a later stage — leave it `None` for now).

### What `has_more` and `load_more` mean

The Scenery has two modes, controlled by which `Lens` callbacks are
registered:

- **Eager mode** — no `total_provider`, no `on_load_chunk`. The
  scenery's row set comes from whatever the cache has. `row_count`
  equals the filtered cache size, `has_more` is always false,
  `set_viewport` / `request_load_more` only emit `ViewportChanged`
  events. Best for caches that fit comfortably in memory and are
  fully populated by `on_start`.
- **Paged mode** — `total_provider` and `on_load_chunk` are both
  registered. `row_count` and `estimated_total` come from the
  provider; `row(i)` returns `None` for indices that haven't been
  fetched yet (your `render_td` paints a skeleton); `set_viewport`
  debounces scroll updates and fires `on_load_chunk` for any
  uncached range.

Inside paged mode there are two paging primitives, picked per master
by inspecting `scenery.master_capabilities()`:

- **Random-access masters** (`can_fetch_page: true`) — SQLite,
  Postgres, CSV, REST with offset. `set_viewport(range)` is the only
  growth primitive needed; dragging the scrollbar to the bottom
  fetches the visible band directly. `load_more` should be a no-op
  here — calling `request_load_more` would march the cache forward
  from index 0 one page at a time, which is wrong when the user has
  already jumped past the cached region.
- **Cursor-only masters** (`can_fetch_next: true`, `can_fetch_page:
  false`) — DynamoDB, token-paginated REST. The master can't be
  queried by row index, so `request_load_more` is the only growth
  primitive: it pages the next `page_size` rows past the cache end.
  `set_viewport` past the cache end is a no-op.

You wire `has_more` / `load_more` / `visible_rows_changed` through to
the Scenery in both modes — the eager-mode no-ops are harmless, and
switching a screen from eager to paged is a Lens-config change with
no UI rewiring. gpui-component virtualizes which rows it *renders* on
its own — only the visible window's `render_td` fires, even if
`row_count()` reports 1,000,000.

## Virtual / infinite scroll: three patterns

Diorama's design is *cache-as-viewport*: the UI shows what's locally
cached, not what could be fetched from master on scroll. Three
patterns, picking the right one for your data size:

### 1. Eager cache + GPUI virtualization — small/medium caches

This is the default and it's the one you almost certainly want. The Lens's
`on_start` (or a streaming `on_refresh`) populates the cache; the Scenery
exposes the full filtered set as `row_count`; gpui-component virtualizes
which rows render.

```rust
let lens = Arc::new(
    Lens::new()
        .cache_at("./products.redb")
        .on_start(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.master().list_values().await?;
                dio.cache().insert_values(rows).await?;
                Ok(())
            }
        })
        .build()?,
);
let dio = lens.make_dio(products_vista).await?;
let scenery = dio.table_scenery().sort("name", SortDir::Asc).open().await?;
```

If the cache has 10k or 100k rows that fit comfortably in memory (most
desktop caches do), you're done. GPUI's table will scroll smoothly because
only the visible band hits `render_td`.

**Sizing.** Each loaded row is one `Arc<EnrichedRecord>` clone. A row with a
few short string columns is on the order of 1 KB allocated; 100k rows ~ 100
MB, which is fine for desktop. Past that, switch strategy.

### 2. Bypass the Scenery for unbounded local data

If your cache is genuinely huge (tens of millions of rows in redb, log
streams, an event store), the eager `Vec<Arc<EnrichedRecord>>` is the wrong
shape. Today, the right escape hatch is to bind GPUI's table to the cache
directly and let your delegate page through it.

The Dio exposes the cache as `dio.cache(): &Arc<dyn CacheTable>` and the
event bus as `dio.subscribe_events()`. Hand-roll the delegate:

```rust
use vantage_diorama::DioEvent;

pub struct LargeCacheTable {
    dio: Dio,
    // Page cache: id range → loaded rows. Sized for ~10× your viewport.
    pages: parking_lot::RwLock<lru::LruCache<usize, Vec<(String, Record<CborValue>)>>>,
    total: AtomicUsize,
    _watch_task: Task<()>,
}

impl LargeCacheTable {
    pub fn new(dio: Dio, cx: &mut Context<Self>) -> Self {
        // Subscribe so we know when to invalidate our page cache.
        let dio_for_task = dio.clone();
        let mut rx = dio.subscribe_events();
        let task = cx.spawn(|this, mut cx| async move {
            while let Ok(evt) = rx.recv().await {
                match evt {
                    DioEvent::RecordChanged { .. }
                    | DioEvent::RecordInserted { .. }
                    | DioEvent::RecordRemoved { .. }
                    | DioEvent::Invalidated => {
                        this.update(&mut cx, |this, cx| {
                            this.pages.write().clear();
                            this.refresh_total(&dio_for_task, cx);
                            cx.notify();
                        }).ok();
                    }
                    _ => {}
                }
            }
        });
        // … kick off initial total fetch …
        Self { dio, pages: Default::default(), total: 0.into(), _watch_task: task }
    }
}

impl TableDelegate for LargeCacheTable {
    fn rows_count(&self, _cx: &App) -> usize {
        self.total.load(Ordering::Relaxed)
    }

    fn render_td(&mut self, row_ix, col_ix, _, cx) -> impl IntoElement {
        let page_ix = row_ix / PAGE_SIZE;
        match self.pages.read().peek(&page_ix) {
            Some(rows) => render_cell(&rows[row_ix % PAGE_SIZE], col_ix),
            None => {
                self.kick_off_page_load(page_ix, cx);
                skeleton_cell(col_ix)
            }
        }
    }

    fn visible_rows_changed(&mut self, range: Range<usize>, _w, cx) {
        for p in (range.start / PAGE_SIZE)..=(range.end.saturating_sub(1) / PAGE_SIZE) {
            if !self.pages.read().contains(&p) {
                self.kick_off_page_load(p, cx);
            }
        }
    }
}
```

`CacheTable` doesn't have a native paginated read today — you'd `list_values()`
once and bucket into pages, or extend the trait. Either way, the contract is
clear: the cache is the source of truth, your delegate is a viewport onto it,
and the event bus tells you when to invalidate the viewport.

`gpui-component`'s `has_more` / `load_more` are not the right hooks for this
shape — they're "load more from a paginated server" hooks. For "I have N
million local rows, render the visible band," you want `visible_rows_changed`
plus your own page LRU.

### 3. Paged Scenery — windowed virtualised grids

The Scenery's paged mode covers the middle ground: caches "big enough
that eager hurts but small enough that a Scenery still makes sense."
Register both Lens callbacks:

```rust
let lens = Arc::new(
    Lens::new()
        .cache_at("./orders.redb")
        .total_provider(|dio| {
            let dio = dio.clone();
            async move { Ok(dio.master().get_count().await? as usize) }
        })
        .on_load_chunk(|dio, range, sink| {
            let dio = dio.clone();
            async move {
                let page = dio
                    .master()
                    .fetch_offset(range.start, range.end - range.start)
                    .await?;
                for (offset, (id, rec)) in page.into_iter().enumerate() {
                    sink.push(range.start + offset, id, rec).await?;
                }
                Ok(())
            }
        })
        .build()?,
);
let dio = lens.make_dio(orders_vista).await?;
let scenery = dio.table_scenery().page_size(200).open().await?;
```

The Scenery:

- Holds a sparse `BTreeMap<usize, Arc<EnrichedRecord>>`.
- Honours `set_viewport(range)` via a 50ms-debounced channel that
  coalesces rapid scroll bursts into a single chunk fetch.
- Honours `request_load_more` to page the next `page_size` rows past
  the cache end — the right primitive for cursor-only masters.
- Surfaces `has_more = true` while the cached map size is below
  `total_provider`'s reported total.
- Emits `DioEvent::RangeLoaded { range }` after each chunk arrives;
  the existing `subscribe()` watch bumps once per chunk.
- Exposes `master_capabilities()` so the `TableDelegate` can branch
  `load_more` between the random-access and cursor-only paths.

The same `TableDelegate` from pattern 1 works unchanged — the only
difference between eager and paged is which Lens callbacks the user
registers. The bypass pattern (2) remains the right call for the
largest workloads where even chunked windowing is overkill.

## Search and sort: in-memory today

`scenery.set_search(Some("cake".to_string()))` and
`scenery.set_sort(Some("price".to_string()), SortDir::Desc)` both work end-to-end:
the Scenery's reload task wakes, re-filters/sorts the cached row set,
publishes a new generation. The widget re-renders.

What's **not** happening yet: push-down to master or to the cache backend.
The filter runs in-memory across the loaded `Vec<Arc<EnrichedRecord>>`.
For 100k rows × a handful of text columns, that's milliseconds. For
significantly larger workloads it'll need vista stage 5b's `add_order` /
`add_search` on the facade Vista.

Wiring a search input:

```rust
input.on_change(cx, move |_this, ev, _cx| {
    let q = ev.text.clone();
    scenery_for_input.set_search((!q.is_empty()).then(|| q));
});
```

No debouncing in the Scenery — if you want it (you probably do), debounce on
the input side.

## `RecordScenery` — the detail sheet

```rust
pub struct RecordSheetView {
    entity: Entity<RecordSceneryEntity>,
    columns: Vec<ColumnSpec>,
}

impl Render for RecordSheetView {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = self.entity.read(cx);
        match entity.scenery().status() {
            RecordStatus::NotFound => return Self::deleted_view(),
            RecordStatus::Error(e) => return Self::error_view(e),
            RecordStatus::Loading => return Self::loading_view(),
            RecordStatus::Stale | RecordStatus::Fresh => {}
        }
        let Some(record) = entity.scenery().record() else {
            return Self::loading_view();
        };
        v_flex().children(
            self.columns.iter().map(|col| field_row(col, &record)),
        )
    }
}
```

Opening the sheet from a row activation:

```rust
fn on_row_double_clicked(&mut self, row: &EnrichedRecord, cx: &mut Context<Self>) {
    let id = row.record.get("id")
        .and_then(|v| v.as_text().map(String::from))
        .unwrap_or_default();
    let dio = self.dio.clone();
    cx.spawn(|this, mut cx| async move {
        let scenery = dio.record_scenery(id).await?;
        cx.update(|cx| {
            let bridge = cx.new(|cx| RecordSceneryEntity::new(scenery, cx));
            this.update(cx, |this, cx| this.open_sheet(bridge, cx)).ok();
        }).ok();
        Ok::<_, vantage_core::VantageError>(())
    }).detach();
}
```

When the same record is mutated elsewhere — a write through `dio.vista()`,
an `on_event` callback handling a SurrealDB LIVE message, an explicit
`dio.invalidate_record(id)` — the bus publishes `RecordChanged { id }`,
this Scenery's reload task notices, re-reads the cache for that id, bumps
generation. The sheet re-renders.

### Cache miss on open

`Dio::record_scenery(id).await?` reads the cache once. If the row isn't
there, status is `RecordStatus::NotFound` and `record()` is `None`. There's
**no master fallback in v1** — the cache is the source of truth on the UI
side. Two ways to seed:

- From an `on_event` callback: `dio.patched(id, record).await?` writes the
  record to cache and publishes `RecordChanged`. Sceneries pick it up.
- From your own code: same call.

If you want the sheet to fetch from master when the cache misses, do it
explicitly in the open handler — it's a few lines and the policy stays
visible.

### What if the record is deleted while the sheet is open?

Bus publishes `RecordRemoved { id }`; the Scenery's reload finds nothing in
cache and flips to `RecordStatus::NotFound`. Your render branch handles it
(close the sheet, show a banner, whatever).

## `ValueScenery` — counters and badges

```rust
let unread = dio.value_scenery()
    .aggregate(Aggregate::CountWhere(vec![
        ("read".to_string(), false.into()),
    ]))
    .open()
    .await?;
```

The builder also exposes `.count()`, `.count_where(conds)`, `.sum(col)`,
`.max(col)`, `.min(col)`, and `.custom(closure)`. `Sum`/`Max`/`Min`
recognise CBOR integers only in v1; floats yield `Error("non-integer field")`
through the status surface (last good value is preserved). For free-form
aggregates use `.custom()` and return whatever CBOR you want.

```rust
impl Render for UnreadBadge {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.entity.read(cx).scenery().value()
            .and_then(|v| match v {
                CborValue::Integer(i) => i128::from(i).try_into().ok(),
                _ => None,
            })
            .unwrap_or(0i64);
        if count == 0 { return None; }
        Some(badge(count.to_string()))
    }
}
```

The Scenery re-evaluates the aggregate against the cache on every relevant
bus event and bumps generation only when the result actually changes
(equality on the resulting `CborValue`). The badge animates "3" → "4"
without you writing the diff.

## Log viewer pattern

The vantage-ui log viewer is currently push-based (`append`, `extend`,
`LinesAppended` events) and stands outside the Vista world. Two ways to
integrate:

- **Keep it as-is.** The widget is fine standing alone. Don't model log
  lines as Vista records.
- **Model lines as a TableScenery over an append-only log Vista** (e.g.
  `vantage-log-writer`). New lines arrive via the Dio's event bus as
  `RecordInserted`. The Scenery's reload picks them up. Bigger lift — only
  worth it if you want unified row-status semantics with the rest of the
  app.

Either choice is reasonable. Pattern 1 from "Virtual / infinite scroll"
above also works directly against a log Vista with no Scenery at all.

## Where the adapter code lives

UI-framework-specific glue lives in `vantage-ui-adapters`. The Diorama GPUI
adapter lands there alongside the existing shared types:

```
vantage-ui-adapters/src/diorama/
├── mod.rs
├── table_entity.rs        // TableSceneryEntity
├── record_entity.rs       // RecordSceneryEntity
├── value_entity.rs        // ValueSceneryEntity
└── delegate.rs            // example delegates
```

Cursive, slint, egui, tauri adapters slot in as sibling modules. Each is
20–80 lines: define the framework-native entity, bridge the watch channel,
delegate to the Scenery.

This module isn't written yet (see
[plans/8-gpui-adapter.md](plans/8-gpui-adapter.md)). The patterns above
describe what it will contain; you can hand-roll them today against the
Scenery trait directly.

## Pull-on-render performance

Knobs to be aware of:

- **`row(idx)` allocates an `Arc::clone`.** Cheap, but with N visible cells
  per render frame it's N clones. GPUI virtualizes rendering so N is small
  (~50 rows × columns) regardless of `row_count`.
- **`set_viewport` is called frequently** by gpui-component (every scroll
  tick). The Scenery debounces internally (50ms by default — tunable via
  `LensBuilder::viewport_debounce`), so a rapid scroll burst collapses
  into a single `on_load_chunk` call against the most recent range.
  Your delegate impl should still keep the call cheap; it's just an
  mpsc send.
- **Lazy field formatting.** If your `render_cell` does heavy string work
  (number formatting, date parsing, markdown), cache it on `EnrichedRecord`
  load rather than on every render. Profile before optimizing.

If the grid feels janky, the usual culprit is `render_td`, not the Scenery.

## A worked example end-to-end

Products grid with quicksearch, sortable columns, double-click to open a
detail sheet that auto-refreshes.

```rust
use std::sync::Arc;
use gpui::{Context, Entity, Window};
use vantage_diorama::{Dio, scenery::{SortDir, TableScenery}};

pub struct ProductsView {
    dio: Dio,
    scenery: Arc<dyn TableScenery>,
    grid: Entity<ProductsGrid>,
    search_input: Entity<TextInput>,
}

impl ProductsView {
    pub async fn new(dio: Dio, cx: &mut Context<Self>) -> Result<Self> {
        let scenery = dio.table_scenery()
            .sort("name", SortDir::Asc)
            .open()
            .await?;

        let bridge = cx.new(|cx| TableSceneryEntity::new(scenery.clone(), cx));
        let grid = cx.new(|cx| ProductsGrid::new(bridge, product_columns()));

        let dio_for_open = dio.clone();
        cx.subscribe(&grid, move |_, _, ev: &RowActivated, cx| {
            let dio = dio_for_open.clone();
            let id = ev.id.clone();
            cx.spawn(|this, mut cx| async move {
                let scenery = dio.record_scenery(id).await?;
                cx.update(|cx| {
                    let bridge = cx.new(|cx| RecordSceneryEntity::new(scenery, cx));
                    this.update(cx, |this, cx| this.open_sheet(bridge, cx)).ok();
                }).ok();
                Ok::<_, vantage_core::VantageError>(())
            }).detach();
        });

        let scenery_for_search = scenery.clone();
        let search_input = cx.new(|cx| {
            let mut input = TextInput::new(cx);
            input.on_change(cx, move |_, ev, _cx| {
                let q = ev.text.clone();
                scenery_for_search.set_search((!q.is_empty()).then(|| q));
            });
            input
        });

        Ok(Self { dio, scenery, grid, search_input })
    }
}
```

The grid renders rows currently in the cache. The search input filters live
across the cached set. Double-click opens an auto-refreshing sheet. When any
record under `dio` changes — locally via `dio.vista().insert()`, externally
via `dio.handle_event(...)` → `on_event` → `dio.patched(...)`, or wholesale
via `dio.refresh().await` — the relevant Sceneries bump generation, the
bridges call `cx.notify()`, the UI updates.

You wrote zero synchronization code. What you did write is the cache
strategy (the Lens), the column definitions, and the render functions.
That's the whole point.
