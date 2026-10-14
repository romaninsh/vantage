# Wiring Up a UI

This is for someone connecting Diorama to a UI framework. The running example
is GPUI with `gpui-component`'s `Table`, but nothing here is GPUI-specific:
any framework that pulls rows on render and has some way to schedule a repaint
(slint, cursive, egui, ratatui, tauri) binds the same way.

The goal is a grid that shows rows as they arrive and repaints when they
change, lets the user scroll, search and sort, plus a detail sheet that follows
its record and a counter that follows an aggregate.

## What Diorama gives a UI

A UI never talks to the master Vista. It opens views on a Dio and reads them:

- **Reads are synchronous.** `row(i)`, `row_count()` and `view_stats()` read
  memory the view already holds. A render frame never awaits.
- **Writes go through `dio.flash(..)`.** The value is staged in the cache and
  shown at once; the write-through runs behind it and either confirms or rolls
  back.
- **Changes arrive on the Dio's event bus.** Each view reacts to the events
  that concern it and bumps its `Generation`. The UI watches the generation and
  repaints.

What to fetch, from where and when is the Lens's business (see
[README_lens.md](README_lens.md)). The UI asks one question: what does this
view hold right now?

## The three sceneries

Every widget binds to one of three views, all exported from the crate root:

- **`TableScenery`**: an ordered set of rows, for a grid, a list, a picker.
  Opened with `dio.table_scenery()…open().await?`.
- **`RecordScenery`**: one row by id, for a detail sheet. Opened with
  `dio.record_scenery(id)…open().await?`.
- **`ValueScenery`**: one value computed over the Dio's rows, for a counter or
  a badge. Opened with `dio.value_scenery(aggregation).open().await?`.

```rust
use vantage_diorama::aggregate::{Conditions, Count, Where};
use vantage_vista::SortDirection;

let grid = dio
    .table_scenery()
    .order("price", SortDirection::Ascending)
    .open()
    .await?;

let sheet = dio.record_scenery("sku-1234").open().await?;

let unread = dio
    .value_scenery(Where::new(Conditions::new().eq("unread", true), Count::rows()))
    .open()
    .await?;
```

Each handle is an `Arc<dyn …>`, cheap to clone and cheap to drop. A scenery
holds its Dio weakly, so it doesn't keep the Dio alive: keep the `Dio` in your
application state for as long as its views are on screen. A view whose Dio has
gone keeps answering with what it last held and stops updating.

## Owned views over shared rows

Every `open()` returns a view of its own. Two widgets that open the same query
(the same filters, search, order and `titles_only`) get two views, but the views
read one set of rows: one reactor on the event bus, one viewport loop, one
fetch per window and one count, however many views show them.

```rust
let grid = dio.table_scenery().page_size(50).open().await?;
let sidebar = dio.table_scenery().page_size(50).open().await?;
grid.set_viewport(0..50, 0..50);
sidebar.set_viewport(0..50, 0..0);
// One fetch for 0..50 reaches the master; both views show its rows.
```

A setter (`set_order`, `set_search`, `set_filters`) changes only the view it is
called on. The view moves to the rows of its new query: it joins them if
another view of that query is open, and otherwise gets a copy of the rows it
leaves, which the new query's load then replaces. The rows it leaves are never
reshaped, so every other view of the old query is untouched.

```rust
let first = dio.table_scenery().open().await?;
let second = dio.table_scenery().open().await?;
first.set_order(Some(("name".into(), SortDirection::Descending)));
first.set_filters(vec![Condition::new("name", FilterOp::Ne, "beta")]);
// `second` still shows every row, in the Dio's order.
```

### Copy-on-write

The copy is what keeps a re-pointed view on screen. It starts as the old rows,
with their places, their total and their settled state, so the view doesn't
blank and doesn't report `Loading` while the new query's rows are on their
way. The load then overwrites the copy in place, the way a paged refetch
overwrites slots as they land.

Two consequences are worth knowing:

- A filter change drops the copied total, since it counted the old filter's
  set. An order or search change keeps it until the reload restates it.
- On a view that pages windows, the reload refetches the window the view shows.
  Copied rows outside that block keep the positions they had in the old order
  until they are fetched again, which happens as the user scrolls to them.

### Why not let a setter reshape the shared rows?

Because a second widget reads those rows. A sort on one grid would reorder a
sidebar built over the same query, and a search typed into a picker would
narrow every other picker on the form. Each view owning its clauses is the only
way two widgets can agree on rows without agreeing on everything else.

The cost is one load per distinct query. A view that re-points to a query no
other view holds pays for its own fetch, and the copy is what hides that fetch
from the user.

### Lifecycle

A view holds its rows strongly and the Dio's registry holds them weakly, so the
rows live exactly as long as some view points at them. When the last view
drops, the rows' work is aborted on the spot: the reactor, the viewport loop
with any load it has in flight or in its debounce, and a two-pass list pass. A
closed grid stops pulling. There is nothing to call; drop the handle.

Opening the same query again after that builds new rows and loads again. With
the `diagnostics` feature, `dio.diagnostics()` lists one entry per open view,
with `shared_by` counting the views that read the same rows:

```rust
let d = dio.diagnostics().await?;
assert_eq!(d.sceneries.len(), 2);
assert!(d.sceneries.iter().all(|s| s.shared_by == 2));
drop(grid);
drop(sidebar);
assert!(dio.diagnostics().await?.sceneries.is_empty());
```

## Opening a table view

The builder fixes what the view opens with:

| Builder call | What it does |
|---|---|
| `filter(condition)` | Narrows the view for its whole life. Repeatable; ANDed with the runtime filters. |
| `search(text)` | The initial quicksearch, as a later `set_search` would set it. Blank means none. |
| `order(column, dir)` | The view's order. Without one the view keeps the Dio's order. |
| `limit(n)` | Show at most the first `n` rows of the order. |
| `tail(n)` | Show the last `n` rows of the order, still in that order. |
| `page_size(n)` | The page `request_load_more` and the on-open fetch use. Default 100. |
| `titles_only()` | The picker shape: list columns only, no detail pass on an augmented Dio. |
| `columns([..])` | The columns this view shows; gates the augment detail pass. |
| `sugar(sugar)` | Read rows with display-only columns merged in. The view never shares rows. |

`open()` fails for `limit` together with `tail`, and for a `tail` with no order
to take, its own or the Dio's.

### Limit and tail

`limit(n)` reads complete at `n`: `has_more()` turns false and the total stops
there.

`tail(n)` is the log-tail shape. It counts back from the end of the set, so it
needs to know where the end is:

- Over a complete set (an eager view, or a two-pass view whose list pass has
  listed to the end) the end is always known, and nulls and ties sit exactly
  where the declared order puts them.
- Over windows, the end is known once the source states a total or a short
  page proves it, and the tail then asks for the last `n` rows.
- A source that pages windows and never states a total has no end to count
  from. Its first fetch is the tail's whole cap in the declared order, and when
  that fetch shows the end is a horizon the tail turns flipped: it reads the
  first `n` rows of the reversed order, backwards. Where nulls and ties fall is
  then the master's call.

Until the tail knows which case it is in, it shows no rows and reads
`LoadState::Loading`, so it never shows rows from the wrong end. A two-pass
tail reads `Loading` the same way during a re-sort, until the list pass has
listed the new order to the end.

`set_order(None)` on a tail falls back to the Dio's order. With no Dio order
either, the view fails (`LoadState::Failed`) and shows nothing until an order
is set again.

### Runtime setters

- `set_order(Some((column, dir)))` replaces the order; `None` clears it.
- `set_search(Some(text))` replaces the quicksearch; `None` or blank clears.
- `set_filters(conditions)` replaces the runtime filters, ANDed with the
  builder's. An empty vec clears. Restating the same set is not a change.

Whether a clause can be answered depends on where the view's rows come from.
A paged view sends every clause into each window fetch, and the master takes
what it can. What it leaves over is evaluated over a whole copy of the set when
the Lens has an eager start for that (`Loader::paged(..).or_eager(..)`), and
otherwise fails the view loudly rather than showing a filtered window as the
filtered set. An eager view evaluates everything over its complete cache. Ask
before you offer the affordance:

- `search_supported()` says whether `set_search` will do something honest.
- `filters_supported(&conditions)` says the same for a filter set.
- `count_where(conditions)` counts the rows a filter set would leave, for a
  chip that shows its count before it is applied. `Ok(None)` means no honest
  count (a two-pass view, or a paged view whose master can't take the filter
  and has no whole copy to count).

## The pull-on-render contract

GPUI's `TableDelegate` is polled on every frame. The parts that matter here
(see `gpui-component`'s `table/delegate.rs`):

```rust
fn rows_count(&self, cx: &App) -> usize;
fn render_td(&mut self, row_ix: usize, col_ix: usize, window: &mut Window,
             cx: &mut Context<TableState<Self>>) -> impl IntoElement;
fn has_more(&self, cx: &App) -> bool;
fn load_more(&mut self, window: &mut Window, cx: &mut Context<TableState<Self>>);
fn visible_rows_changed(&mut self, range: Range<usize>, window: &mut Window,
                        cx: &mut Context<TableState<Self>>);
```

Two rules follow:

1. **Reads are cheap and synchronous.** `row(i)` clones an
   `Arc<EnrichedRecord>` out of the view's row map. It returns `None` for a
   row that hasn't loaded yet; paint a placeholder.
2. **Updates push, the UI pulls.** A change bumps the view's generation. The
   adapter turns that into a repaint request, the framework re-renders, and the
   delegate reads the view again. The view never tells the UI which cell
   changed.

### `set_viewport(loaded, shown)`

The view needs two ranges, both in its own indices:

- `loaded`: the rows to load, and to hydrate on an augmented Dio.
- `shown`: the rows on screen. It is the only input to `ViewStats::showing`,
  the "Showing 41–60 of 1,204" a status bar prints. An empty range means none
  on screen.

A grid passes the visible band twice. A dashboard counter that only needs rows
loaded, or a terminal table that hydrates a band around the cursor while
showing the whole screen, passes different ranges:

```rust
scenery.set_viewport(visible.clone(), visible);
scenery.set_viewport(0..200, 0..0);
scenery.set_viewport(band_start..band_end, offset..offset + height);
```

Calls are cheap. The view records the range and queues a request, and the
viewport loop coalesces a scroll burst into one fetch per view, after
`Tuning::viewport_debounce` (50 ms by default) of quiet.

### `row_by_id`

`row_by_id(id)` finds a row wherever the view's order puts it, which is what a
selection or a "scroll to the row I just edited" needs after a re-sort. It
returns `None` when the view doesn't hold the row: not loaded, filtered out, or
outside a `limit` or `tail`.

### `view_stats`

`view_stats()` is one snapshot of what the view shows: `total` and whether it
is exact, `loaded`, `showing`, `has_more`, the order, search and filters (each
filter tagged with whether the builder or the runtime set it), and a summary
`state`. Reading it clones the last published snapshot; it never fetches.

`count()` turns the total into a `ViewCount` a status bar can print honestly,
and `count_text()` renders it:

| `ViewCount` | `count_text()` | When |
|---|---|---|
| `Exact(1204)` | `1,204` | the source stated the total, or the set is complete |
| `Estimated(500)` | `~500` | a total remembered from an earlier session, not yet restated |
| `AtLeast(100)` | `100+` | no total, and more rows to fetch: the loaded rows are a lower bound |
| `Loaded(37)` | `37` | no total, and nothing more to fetch: the loaded rows are all there is |

`subscribe_view_stats()` ticks when the snapshot changes. It is a separate
channel from `subscribe()`: a viewport move changes the snapshot without
changing a row, and a row hydrating changes a row without changing the
snapshot.

### The status enums are open

`LoadState`, `RowStatus`, `RecordStatus`, `ValueStatus`, `ViewState`,
`ServoStatus` and `DioEvent` are all `#[non_exhaustive]`. A match on one needs a
`_ =>` arm, and that arm should be your "not my concern" path, not a panic:

```rust
match scenery.load_state() {
    LoadState::Loading => skeleton(),
    LoadState::Complete if scenery.row_count() == 0 => empty_placeholder(),
    LoadState::Failed(failure) => error_banner(&failure.message),
    _ => grid(),
}
```

`Loading` and an empty `Complete` render the same blank grid and mean opposite
things; telling a user the table is empty while the first fetch is in flight is
the bug this distinction exists to prevent.

## The GPUI binding

For each scenery the bridge is the same: an entity that holds the handle and a
task that watches its generation and calls `cx.notify()`.

```rust
pub struct TableSceneryEntity {
    scenery: Arc<dyn TableScenery>,
    _watch: Task<()>,
}

impl TableSceneryEntity {
    pub fn new(scenery: Arc<dyn TableScenery>, cx: &mut Context<Self>) -> Self {
        let generation = scenery.subscribe();
        let watch = cx.spawn(async move |this, cx| {
            let mut seen = *generation.borrow();
            loop {
                cx.background_executor().timer(Duration::from_millis(50)).await;
                let now = *generation.borrow();
                if now != seen {
                    seen = now;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        return;
                    }
                }
            }
        });
        Self { scenery, _watch: watch }
    }

    pub fn scenery(&self) -> &Arc<dyn TableScenery> {
        &self.scenery
    }
}
```

The task probes the generation on a short timer instead of awaiting
`changed()` on GPUI's executor. The views bump from tokio tasks on other
threads, and an await on the foreground executor can miss that wake;
vantage-ui's `WatchLoop` probes the same way and uses `changed()` only to cut
the sleep short. Dropping the entity drops the task, which drops the receiver.
`RecordSceneryEntity` and `ValueSceneryEntity` are the same with a different
trait object. GPUI's `spawn` signature moves between releases; the shape above
is the part that matters.

## A `TableDelegate` over a `TableScenery`

```rust
use vantage_vista::{Capability, SortDirection};

impl TableDelegate for ProductsTable {
    fn rows_count(&self, cx: &App) -> usize {
        self.entity.read(cx).scenery().row_count()
    }

    fn render_td(&mut self, row_ix: usize, col_ix: usize, _: &mut Window,
                 cx: &mut Context<TableState<Self>>) -> impl IntoElement {
        let column = &self.columns[col_ix];
        match self.entity.read(cx).scenery().row(row_ix) {
            Some(row) => render_cell(&row, column),
            None => placeholder_cell(column), // not loaded yet
        }
    }

    fn visible_rows_changed(&mut self, range: Range<usize>, _: &mut Window,
                            cx: &mut Context<TableState<Self>>) {
        self.entity.read(cx).scenery().set_viewport(range.clone(), range);
    }

    fn has_more(&self, cx: &App) -> bool {
        self.entity.read(cx).scenery().has_more()
    }

    fn load_more(&mut self, _: &mut Window, cx: &mut Context<TableState<Self>>) {
        let scenery = self.entity.read(cx).scenery();
        // A windowed master is driven by `set_viewport` alone.
        if !scenery.master_capabilities().has(Capability::Window) {
            scenery.request_load_more();
        }
    }

    fn perform_sort(&mut self, col_ix: usize, sort: ColumnSort, _: &mut Window,
                    cx: &mut Context<TableState<Self>>) {
        let field = self.columns[col_ix].field.clone();
        let order = match sort {
            ColumnSort::Ascending => Some((field, SortDirection::Ascending)),
            ColumnSort::Descending => Some((field, SortDirection::Descending)),
            ColumnSort::Default => None,
        };
        self.entity.read(cx).scenery().set_order(order);
    }
}
```

`render_cell` and `placeholder_cell` are yours. An `EnrichedRecord` carries the
row (`record`), its `status` and `fetched_at`. Render the status: `Incomplete`
for an augmented row whose detail hasn't arrived, `PendingWrite` while a save
is in flight, `WriteFailed` and `LoadFailed` with their errors.

`request_load_more()` pages the next `page_size` rows past what is loaded,
which a source that can only walk forward needs; over a windowed master,
`set_viewport` already fetches whatever band the user jumped to, and a
load-more would only march forward from the top. On a two-pass view it lists
the next list page instead. A `limit` that is full and a `tail` (which asks for
its whole cap) ignore it.

## Where the rows come from

Which shape a view has is the Lens's choice, made per master when the view
opens, and the same delegate drives all of them:

- **Eager.** The Lens copies the whole set into the cache
  (`Loader::eager`). The view seeds from the cache, `has_more()` is false, and
  search, sort and filters run in memory over the complete set.
- **Paged.** The Lens fetches windows as views ask (`Loader::paged`).
  `row(i)` is `None` until its window lands; the total comes from what the
  source states, a short page, or a horizon one page past the last full one.
  `Loader::paged(..).or_eager(..)` pages a master that advertises `Window`
  and copies one that doesn't, so one Lens serves both.
- **Two-pass.** On a Dio built with `augment(..)`, a list pass lists cheap
  rows into a shared ordered index and a detail pass hydrates the rows in
  `loaded`, one id at a time, through the Dio's augment scheduler. Rows read
  `Incomplete` until hydrated.

With `Tuning::refresh_on_open` (the default), a paged view re-fetches its first
page from the master when it opens, even over a warm cache: the cache is keyed
by id, and only the master knows the query's order.

## Search and sort

On an eager view, search and sort run over the complete cache. On a paged view
they travel with every window fetch. Either way `set_search` and `set_order`
re-point the view at once and the reload fetches behind the copy, so the grid
never blanks.

`set_search` re-points on every call, and each re-point builds a copy. The
copies a keystroke leaves behind are dropped with their pending fetch, so only
the last one reaches the master, but debouncing the text input still saves the
copies:

```rust
input.on_change(cx, move |_, ev, _| {
    let q = ev.text.clone();
    scenery_for_input.set_search((!q.is_empty()).then_some(q));
});
```

On a two-pass view a sort is a soft refresh. The view re-points at the ordered
index for the new order, keeps the rows it shows, and restarts the detail pass
for its window without waiting for a scroll. An order used before re-sorts from
its index with no list call; a new one lists its first page.

Two ordering details of two-pass views:

- A view with no client-side refinement, listed from a whole copy in the
  cache, takes the cache's id order. A cold open lists the master and takes the
  master's order, so a cold and a warm open of the same query can differ.
- A refined view (one whose filters, search or order name augmented values,
  and so are evaluated over the cache) re-derives its rows in one batch per
  `Tuning::viewport_debounce` after a row changes. It shows a `RecordChanged`
  up to one debounce later than a plain view does.

## Dropdowns and autocomplete

A dropdown and an autocomplete are the same table view, projected to the title
columns with `titles_only()`:

```rust
let countries = countries_dio.table_scenery().titles_only().open().await?;
// The dropdown renders `countries.row(i)`'s title and submits its id.
// The autocomplete forwards keystrokes and its visible band:
countries.set_search((!q.is_empty()).then_some(q));
countries.set_viewport(visible.clone(), visible);
```

Everything above applies, including shared rows: eight pickers over the same
lookup on one form cost one set of rows. On an augmented Dio `titles_only()`
suppresses the detail pass, so a 10,000-row lookup opens as cheaply as it
lists. A `titles_only` picker and a full grid over the same query read distinct
rows: the grid hydrates and the picker doesn't, so neither drags its cost onto
the other.

## `RecordScenery`: the detail sheet

```rust
impl Render for RecordSheetView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scenery = self.entity.read(cx).scenery();
        match scenery.status() {
            RecordStatus::NotFound => return Self::gone_view(),
            RecordStatus::Loading => return Self::loading_view(),
            RecordStatus::Error(e) => return Self::error_view(&e),
            _ => {}
        }
        let Some(row) = scenery.record() else {
            return Self::loading_view();
        };
        v_flex().children(self.columns.iter().map(|col| field_row(col, &row)))
    }
}
```

A record scenery opens on the cache once, then follows the id:

| Situation | Status |
|---|---|
| the cache holds the row | `Fresh` |
| the row waits for its detail pass (augmented Dio) | `Incomplete`, then `Fresh` |
| the cache misses | `NotFound` |
| the cache misses, opened with `load_on_miss()` | `Loading`, then `Fresh` or `NotFound` |
| the row changes, by a write or an applied change | reloads, stays `Fresh` |
| the row is removed | `NotFound` |
| a reload, a fetch or the detail pass fails | `Error(_)`, the last row still shown |

A row outside the Dio's residual is `NotFound`, as the grid wouldn't show it
either.

The builder has three options:

- `seed(row)` starts from a row you already hold, `Fresh`, without reading the
  cache: a grid handing its current row to the sheet it opens. With `sugar`, or
  on an augmented Dio, the cache's copy then replaces it in the background to
  bring the sugar columns, or `Incomplete` and the detail pass. When the cache
  doesn't hold the id, the seeded row stays as seeded until the id changes.
- `load_on_miss()` fetches a missing row from the master. A row the Dio's
  residual admits is cached and shown; an absent one, or one outside the
  residual, reads `NotFound` and caches nothing.
- `sugar(sugar)` reads the row with display-only columns merged in.

Opening the sheet from a row activation:

```rust
fn on_row_activated(&mut self, row: &EnrichedRecord, cx: &mut Context<Self>) {
    let Some(id) = row.record.get("id").and_then(|v| v.as_text()).map(String::from) else {
        return;
    };
    let dio = self.dio.clone();
    let handed = row.record.clone();
    cx.spawn(async move |this, cx| {
        let sheet = dio.record_scenery(id).seed(handed).open().await?;
        this.update(cx, |this, cx| this.open_sheet(sheet, cx))?;
        Ok::<_, anyhow::Error>(())
    })
    .detach();
}
```

## Editing

A save is a flash. `ChangeFlash::partial(id, fields)` patches only the fields
the form changed:

```rust
let dio = self.dio.clone();
let id = self.id.clone();
let edited = self.form.changed_fields(); // Record<CborValue>
cx.spawn(async move |_, _| {
    // Err after a rollback; the row already shows WriteFailed.
    let _ = dio.flash(ChangeFlash::partial(id, edited)).await;
})
.detach();
```

The value is staged in the cache and shown at once, and the row's status flips
to `RowStatus::PendingWrite`. On success it settles to `Fresh`. On failure the
cached row is restored and its status is `RowStatus::WriteFailed { error }`, so
the form reverts instead of showing a value that didn't save. Every view reads
the one cached row, so the grid row, a second sheet and a badge all update
together:

```rust
match &row.status {
    RowStatus::PendingWrite => spinner("saving…"),
    RowStatus::WriteFailed { error } => error_badge(error),
    _ => normal_cell(row),
}
```

For a form, open a `Servo` instead of patching by hand: `dio.servo(id).await?`
tracks the record, `servo.set(field, value)` holds the user's edits against
upstream changes, and `servo.flash().await?` sends only the changed fields.
`ServoStatus::Failed` keeps the draft and carries per-field errors when the
write path named them. A servo holds a strong Dio handle, so the write pipeline
stays up while the form is open.

## `ValueScenery`: counters and badges

```rust
let unread = dio
    .value_scenery(Where::new(Conditions::new().eq("read", false), Count::rows()))
    .open()
    .await?;

impl Render for UnreadBadge {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = match self.entity.read(cx).scenery().value() {
            Some(CborValue::Integer(i)) => i64::try_from(i128::from(i)).unwrap_or(0),
            _ => 0,
        };
        (count > 0).then(|| badge(count.to_string()))
    }
}
```

The scenery opens holding a value computed over the Dio's rows, recomputes as
they change, and bumps its generation only when the result differs, so the
badge goes from 3 to 4 without you writing the diff. A burst of changes
collapses into one recomputation and one trailing flush
(`Tuning::aggregate_debounce`). `status()` reads `Loading`, `Fresh` or
`Error(_)`; an error keeps the last good value. `refresh_source()` asks the Dio
to re-read its master. The aggregations themselves are covered in
[README_models.md](README_models.md#aggregates).

## A log tail

A log viewer is a `tail(n)` over a log Vista ordered by time. The tail keeps
showing the newest `n` rows as inserts arrive at the end:

```rust
let log = log_dio
    .table_scenery()
    .order("at", SortDirection::Ascending)
    .tail(500)
    .open()
    .await?;
```

## Unbounded local data

A view holds the rows it has loaded in memory, about one `Arc<EnrichedRecord>`
each. For a cache with tens of millions of rows that only ever show a screen at
a time, skip the scenery and page the Dio's facade Vista yourself. The facade
advertises `Window` and serves windows from the cache, and the event bus tells
you when to drop your pages:

```rust
let page = dio.cached().fetch_window(offset, PAGE_SIZE).await?;

let mut events = dio.subscribe_events();
cx.spawn(async move |this, cx| {
    while let Ok(event) = events.recv().await {
        match event {
            DioEvent::RecordChanged { .. }
            | DioEvent::RecordInserted { .. }
            | DioEvent::RecordRemoved { .. }
            | DioEvent::DatasetChanged => {
                let _ = this.update(cx, |this, cx| {
                    this.pages.clear();
                    cx.notify();
                });
            }
            _ => {}
        }
    }
})
.detach();
```

You give up what the view did for you: shared rows, statuses, the copy on
re-sort. Reach for it when the memory is the problem, not before.

## Where the adapter code lives

Framework glue lives in `vantage-ui-adapters`, which other UI crates share.
`src/ratatui_dio.rs` is the Diorama binding there today: a terminal table over
a `TableScenery`, with `ValueScenery` counters in its status bar. It forwards
each generation channel into one repaint tick and hydrates a band around the
cursor with `set_viewport`. The other adapter modules bind plain grids. A
binding for another framework is the same three parts: an entity holding the
handle, a generation watch that schedules a repaint, and a delegate that reads
the view.

## Performance

- `row(i)` is an `Arc` clone. A virtualized grid calls it for the visible band
  only, around 50 rows times the column count per frame, whatever
  `row_count()` says.
- `set_viewport` runs on every scroll tick. The view coalesces the burst; keep
  your own call cheap.
- Heavy cell formatting (numbers, dates, markdown) belongs in a cache of your
  own keyed by id and generation, not in `render_td`.

When a grid feels slow, measure `render_td` first. With the `diagnostics`
feature, `LensBuilder::debug_datasource(name)` prints what each view fetched,
coalesced and served from cache; see [ARCHITECTURE.md](ARCHITECTURE.md#debug-stream).

## Checklist

- Keep the `Dio` alive in app state; views hold it weakly.
- One `open()` per widget; never share a view between two widgets that sort or
  search independently.
- Forward the visible band as `set_viewport(visible.clone(), visible)`.
- Paint a placeholder for `row(i) == None` and a skeleton for `Loading`; show
  the empty state only for an empty `Complete`.
- Give every status match a `_ =>` arm.
- Ask `search_supported()` and `filters_supported()` before offering the
  control.
- Save with `dio.flash(..)` or a `Servo`, and render `PendingWrite` and
  `WriteFailed`.
- Drop the handle to close a view; the fetches stop with it.
