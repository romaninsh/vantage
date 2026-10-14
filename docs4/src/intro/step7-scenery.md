# Scenery — Reactive Views

Chapter 6's CLI asks for one window of details and exits — the *asking* is still on the
programmer. A real interface is a standing view: the user scrolls, and the window should follow;
details land one by one, and the rows should repaint; the bucket changes, and the listing should
notice. And it has to hold up at scale — the full GHCN archive is about **122,000** station
files, of which a screen shows forty. What a UI actually needs is an ordered row set it can read
*by index*, hydration that follows the user's attention, and a signal whenever anything on
screen changes.

**Scenery** is that layer. Each Scenery is a reactive view onto a Dio, exposing one access
pattern:

- [`TableScenery`](vantage_diorama::TableScenery) — ordered rows by index, with a viewport
- [`RecordScenery`](vantage_diorama::RecordScenery) — a single record by id
- [`ValueScenery`](vantage_diorama::ValueScenery) — a computed scalar (count, sum, custom)

All three share one reactivity mechanism: a [`Generation`](vantage_diorama::Generation) counter
that bumps whenever the view's state changes. Consumers `subscribe()` to a `watch` channel and
redraw on each bump — the channel only ever holds the *latest* generation, so a burst of changes
costs one repaint, not one per change.

```admonish info title="Why 'Scenery'?"
Point a camera at a vista and you never capture the whole of it. The lens frames a scene: a
limited cut, chosen by where you aim. But what it frames is *alive* — pan, and the scene
follows; wait, and the light changes in front of you. A Scenery makes the same trade. It will
never hold the entire Vista — one window of rows, one record, one number — yet within that
frame everything flows: rows repaint as details land, the count ticks as data arrives, the
window follows the user's attention. Limited, but dynamic — that is the whole design.
```

A Scenery is not another handle to your data — it hands you no records to keep. It maintains
one particular *view* of the Dio's copy, precomputed and ready for a widget to read:

|               | `Dio`                                            | `Scenery`                                              |
| ------------- | ------------------------------------------------ | ------------------------------------------------------ |
| **Purpose**   | Keep a live local copy of a data segment         | Present that copy to a consumer, one access pattern at a time |
| **Shape**     | Master + cache + write queue + event bus         | Ordered rows / one record / one scalar                 |
| **Reads**     | You ask — the facade answers from the cache      | Precomputed — `row(idx)` / `value()` return instantly  |
| **Changes**   | Announces them on the event bus                  | Reacts to the bus, recomputes, bumps its generation    |
| **Queries**   | The master's, carried by the facade              | Its own: sort, search, filter, viewport — served locally |
| **How many**  | One per data segment                             | Many per Dio — one per open, rows shared per query     |
| **Lifecycle** | Long-lived; owns cache and background tasks      | Opened by a view, dropped with it                      |
| **Consumer**  | Loaders, handlers, scripts                       | UI widgets, bound through an *adapter*                 |

That last cell is where this chapter ends up. The consumer — a terminal table here, a desktop
grid or a remote client such as a React page elsewhere — drives its scenery in one repeating
loop:

1. The consumer declares what it shows: `set_viewport(40..80, 40..80)` — nothing more than
   "these rows are on screen."
2. The scenery turns that into work for the Dio: list pages for the spine, detail fetches for
   the rows in view.
3. Results land in the Dio's cache, and the event bus announces each one — `RecordChanged`,
   row by row.
4. The scenery reacts to the bus: it updates the affected slot and bumps its `Generation`.
5. The consumer's watch channel ticks; it re-reads `row(idx)` for what's visible and repaints.

The code that runs this loop for one particular consumer is the **adapter** — the same handful
of lines whether it paints a terminal, a desktop toolkit, or a wire to the browser.

<svg viewBox="0 0 760 250" xmlns="http://www.w3.org/2000/svg" font-family="sans-serif" font-size="13">
  <defs>
    <marker id="arrow7" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="#888"/>
    </marker>
  </defs>

  <!-- Dio container -->
  <rect x="20" y="40" width="160" height="170" rx="12" fill="none" stroke="#8f5a2d" stroke-width="2.5"/>
  <text x="100" y="64" text-anchor="middle" fill="#8f5a2d" font-weight="bold" font-size="15">Dio</text>
  <rect x="40" y="80" width="120" height="40" rx="8" fill="#4a7c59"/>
  <text x="100" y="105" text-anchor="middle" fill="#fff" font-weight="bold" font-size="12">cache</text>
  <rect x="40" y="135" width="120" height="40" rx="8" fill="#4a7c59"/>
  <text x="100" y="160" text-anchor="middle" fill="#fff" font-weight="bold" font-size="12">event bus</text>
  <text x="100" y="228" text-anchor="middle" fill="#888" font-size="11">fetches run through the Lens</text>

  <!-- Scenery container -->
  <rect x="300" y="40" width="160" height="170" rx="12" fill="none" stroke="#7c2d8f" stroke-width="2.5"/>
  <text x="380" y="64" text-anchor="middle" fill="#7c2d8f" font-weight="bold" font-size="15">Scenery</text>
  <rect x="320" y="80" width="120" height="40" rx="8" fill="#7c2d8f"/>
  <text x="380" y="105" text-anchor="middle" fill="#fff" font-weight="bold" font-size="12">rows by index</text>
  <rect x="320" y="135" width="120" height="40" rx="8" fill="#7c2d8f"/>
  <text x="380" y="160" text-anchor="middle" fill="#fff" font-weight="bold" font-size="12">Generation</text>

  <!-- Consumer container -->
  <rect x="580" y="40" width="160" height="170" rx="12" fill="none" stroke="#2d6a8f" stroke-width="2.5"/>
  <text x="660" y="64" text-anchor="middle" fill="#2d6a8f" font-weight="bold" font-size="15">Consumer</text>
  <rect x="600" y="80" width="120" height="95" rx="8" fill="#2d6a8f"/>
  <text x="660" y="108" text-anchor="middle" fill="#fff" font-weight="bold" font-size="12">adapter</text>
  <text x="660" y="130" text-anchor="middle" fill="#fff" font-size="11">UI widget or</text>
  <text x="660" y="147" text-anchor="middle" fill="#fff" font-size="11">remote client</text>

  <!-- 1: consumer → scenery (viewport) -->
  <line x1="576" y1="85" x2="464" y2="85" stroke="#888" stroke-width="2" marker-end="url(#arrow7)"/>
  <text x="520" y="73" text-anchor="middle" fill="#888" font-size="11">① set_viewport</text>

  <!-- 2: scenery → dio (hydrate) -->
  <line x1="296" y1="85" x2="184" y2="85" stroke="#888" stroke-width="2" marker-end="url(#arrow7)"/>
  <text x="240" y="73" text-anchor="middle" fill="#888" font-size="11">② hydrate rows in view</text>

  <!-- 3: dio → scenery (bus) -->
  <line x1="184" y1="185" x2="296" y2="185" stroke="#888" stroke-width="2" marker-end="url(#arrow7)"/>
  <text x="240" y="203" text-anchor="middle" fill="#888" font-size="11">③ RecordChanged</text>

  <!-- 4: scenery → consumer (tick) -->
  <line x1="464" y1="185" x2="576" y2="185" stroke="#888" stroke-width="2" marker-end="url(#arrow7)"/>
  <text x="520" y="203" text-anchor="middle" fill="#888" font-size="11">④ Generation tick</text>

  <!-- 5: consumer → scenery (read) -->
  <line x1="576" y1="135" x2="464" y2="135" stroke="#888" stroke-width="2" stroke-dasharray="5 4" marker-end="url(#arrow7)"/>
  <text x="520" y="123" text-anchor="middle" fill="#888" font-size="11">⑤ row(idx) — repaint</text>
</svg>

## Back to the inventory

Chapter 6 left the CLI listing one prefix and detailing ten fixed rows. What we actually want
from the inventory is the tool you'd reach for daily — and that sets the requirements:

- the **whole archive**, not a prefix — all 122,000 station files, scrollable end to end;
- **instant open**, even on the first run — an empty table that fills as the listing arrives,
  never a frozen prompt;
- **details where the user is looking** — the rows on screen sprout `ROWS` and `LATEST` as
  their files are read, and the cursor drags that attention with it;
- **standing freshness** — the bucket changes, the table follows, plus a grand total of
  readings across the archive.

Measure chapter 6's ending against that. It read from the Dio with
`dio.cached().fetch_window(0, 10)` — and it works fine, for a CLI: the read is bounded to ten
records, and it resolves once all ten have their details. Nothing blocks — it's an async wait,
the runtime stays free — but the *user* waits all the same: twenty seconds of silence on a cold
cache, and every widening of the window widens the wait. For a better experience we need a more
responsive UI:

1. display results as soon as we have them — don't hold rows hostage to their details;
2. show data immediately and fix it reactively as better values arrive;
3. register what the user is currently looking at, reducing what is fetched and transmitted.

The Scenery implements exactly this:

- rows are served as soon as the list pass has them, with `…` standing in for details still
  pending;
- every landed detail updates its own row and bumps the `Generation` — one tick, one repaint;
- `set_viewport(loaded, shown)` registers the user's attention, and only those rows hydrate.

For the consumer, this chapter reaches for **ratatui** — a terminal-UI library — which Vantage
updates in real time through a ready-made adapter. The `dataset-ui-adapters` crate ships such
adapters for several UI frameworks (egui, Slint, GPUI, Cursive, Tauri among them), and they all
speak the same scenery loop. Here is ours a few seconds into a cold start: the first two
visible rows already hydrated, and the total still waiting for the rest of the archive:

```text
FILENAME                              SIZE        ROWS      LATEST
csv/by_station/ACW00011604.csv        41673       1231      19490814
csv/by_station/ACW00011647.csv        481021      14355     20260710
csv/by_station/AE000041196.csv        1998959     …         …
…
 8000 rows · 2 augmented · total rows 0 · ↑/↓ PgUp/PgDn scroll · r refresh · q quit
```

`learn-6` starts as a copy of `learn-5`'s data side — `files.rs`, `readings.rs`, and the whole
Dio setup carry over. Three things change: the prefix widens to the entire archive, the Lens
learns to serve a UI, and the printing loop is replaced by a scenery-bound terminal table.

```sh
cargo add dataset-ui-adapters --features ratatui
```

Widening the scope is two touches. The prefix loses its country filter:

```rust
const PREFIX: &str = "csv/by_station/";
```

And the listing pages grow. Every page is one HTTP round-trip: chapter 5 fetched 100 keys at a
time, a comfortable twelve requests for its 1,122 files — but at 122,000 files the same setting
means 1,220 round-trips of mostly latency. S3 caps a single response at 1,000 keys, so
`files.rs` asks for the cap and the whole archive syncs in about 122 requests:

```rust
# use vantage_aws::prelude::*;
# use vantage_types::EmptyEntity;
# fn table(aws: AwsAccount) -> Table<AwsAccount, EmptyEntity> {
#     Table::new("restxml/Contents:s3/GET /{Bucket}?list-type=2", aws)
#         .with_id_column("Key")
        // S3's per-response maximum — we're listing the whole station set.
        .with_condition(eq("max-keys", 1000))
# }
```

---

## A Lens that serves a UI

Chapter 5's Lens had one job: warm the cache before anyone reads. A UI inverts the priorities —
the screen must appear *immediately*, on whatever is known so far, and data streams in behind
it. Two changes to the builder:

```rust,no_run
# use std::time::Duration;
# use vantage_aws::prelude::*;
# use vantage_diorama::{Cache, Lens, Loader, LoaderCx, Refresher};
# async fn sync(_cx: &LoaderCx) -> vantage_core::Result<()> {
#     Ok(())
# }
# fn run() -> vantage_core::Result<()> {
let lens = Lens::builder()
    .cache(Cache::redb("cache.redb"))
    // Don't wait for the sync: it warms the cache in the background,
    // while the view lists the bucket itself.
    .loader(Loader::eager(|cx: LoaderCx| async move { sync(&cx).await }).blocking(false))
    // Reconcile against the bucket once a minute: new files appear
    // as un-hydrated rows, vanished ones drop out, changed ones are
    // demoted for re-hydration. The augmentation's refresh pass does
    // the work; the refresher only sets the interval.
    .refresher(Refresher::new(|_cx: LoaderCx| async { Ok(()) }).every(Duration::from_secs(60)))
    .build()
    .context("Failed to build lens")?;
# let _ = lens;
# Ok(())
# }
```

**`.blocking(false)`** detaches the sync. An eager start blocks the Dio's `build()` by default,
which is what chapter 5 wanted: nobody read before the cache was warm. Here `build()` returns at
once and the sync runs as a background task, so the UI's first frame never waits on 122
requests. The sync itself is chapter 5's, minus its `println!` narration (a terminal UI owns the
screen now). It writes the cache directly, and writes to the raw cache raise no events, so the
Dio does the announcing: when the detached start finishes it sends `DioEvent::Seeded`, and
every open view re-reads.

That covers the cache. What a `TableScenery` does with it needs a little more context. An
augmented Dio drives **two-pass loading**: a *list pass* fetches cheap rows and establishes the
view's order (its spine), and a *detail pass* hydrates individual rows' augment columns as the
viewport reaches them. The detail pass is chapter 6's augmentation. The list pass picks its
source when a view first lists:

- while the cache isn't known to hold the whole set (a first run, or a sync still running), it
  asks the master, page by page;
- once the Dio holds a **whole copy**, which the eager start records when it finishes, it reads
  the cache instead: one local read, zero network.

So on a cold start the table lists the bucket itself while the sync runs, and on `Seeded` it
re-lists from the warm cache. Nothing in the application chooses between the two; the Dio
knows which answer it can trust.

**The refresher** completes the freshness story. Once a minute the Dio reconciles its cache
against the master (this *does* walk the listing: 122 requests, in the background). New files
appear as un-hydrated rows, deleted ones drop out, and a file whose listing entry changed keeps
showing its old numbers but is marked stale; the next time it's on screen, the detail pass
re-fetches it. Chapter 5 needed `--invalidate` to notice deletions; a standing view just notices.
The refresher's own callback does nothing here: on an augmented Dio the reconcile is the
augmentation's refresh pass, and `Refresher::new(..).every(..)` only sets how often it runs.

### Why not always list from the cache?

Because an empty cache and an empty bucket look the same. A view that read the cache during a
first run would show zero files and call the listing complete, then jump to 122,000 rows
minutes later. A half-written cache is worse: it reads as a complete listing of whichever files
the sync reached first. The whole-copy record is how the Dio tells "the cache is the listing"
from "the cache is what we have so far", and until it can say the first, the master answers.
The cost is the master walk on a cold start, and that walk is the same one the sync is making
anyway.

## Creating the reactive UI

Our UI needs two things from the Dio: the rows, and a grand total. Each one is a scenery.
Let's open them one at a time.

### The table

```rust,no_run
# use vantage_diorama::Dio;
# async fn run(dio: Dio) -> vantage_core::Result<()> {
// One list page covers the whole cached listing (~122k station files);
// the viewport's detail pass hydrates whatever is on screen first.
let scenery = dio.table_scenery().page_size(200_000).open().await?;
# let _ = scenery;
# Ok(())
# }
```

`.open()` builds the scenery's rows (or joins the rows another open of the same query already
built), spawns its reactor (the task that watches the Dio's event bus), and hands back
`Arc<dyn TableScenery>`. What you get is deliberately small and synchronous:

- `row_count()` and `row(idx)`, an [`EnrichedRecord`](vantage_diorama::EnrichedRecord): the
  record plus a per-row status;
- `set_viewport(loaded, shown)`, more on this in a moment;
- `subscribe()`, the generation channel from the start of the chapter;
- `refresh_source()`, which asks the Dio to re-read its master; the view reloads as the fresh
  rows land.

The builder can also chain `.order(column, SortDirection::Ascending)`, `.search(text)` and
`.filter(condition)`, every one of them served locally on a backend that can do none of it. We
don't use them here; the table shows the archive as-is.

```rust,no_run
# use vantage_diorama::Dio;
# use vantage_vista::SortDirection;
# async fn run(dio: Dio) -> vantage_core::Result<()> {
// The same table, from the end of the alphabet.
let reversed = dio
    .table_scenery()
    .order("Key", SortDirection::Descending)
    .open()
    .await?;
# let _ = reversed;
# Ok(())
# }
```

A UI rarely commits to one order at open time, though, and it doesn't have to reopen. The
handle mutates in place: `set_order(Some((column, direction)))` and `set_search(Some(text))`,
bound to a key or a header click, re-point the view at the ordered index for the new query,
swap the visible rows in one atomic step (the grid never blanks mid-reorder), and restart
hydration for whatever is on screen. Sorting back reuses the already-built index: zero list
calls. Runtime filters change the same way, through `set_filters(conditions)`, which takes a
`Vec<Condition>` and replaces the previous set (an empty vec clears it).

One thing probably caught your eye: `page_size(200_000)`?! That's the *list pass*: how many
spine rows one list page asks for. It's a separate axis from the viewport, which drives only
the *detail pass* over rows the spine already holds. Left at the default, the spine would stop
at the first hundred files and nothing in this UI asks for page two. Set past the archive size,
one list page builds the entire spine. Once the Dio holds its whole copy, that page is a single
read of the local cache.

### The viewport

The **viewport** is the scenery's answer to "which rows is the user actually looking at?" — a
plain range of row indexes, like `40..80`. It's declared through `set_viewport(loaded, shown)`, the
method on the handle we just opened. `loaded` is the range to load and hydrate; `shown` is the
range actually on screen, which feeds the view's "showing a–b" statistics and nothing else. A
grid usually passes the same range twice. You rarely call it yourself: whoever renders the
table calls it as the user scrolls. Later in this chapter, that's the ratatui adapter's job.

It is the load-bearing call, because **the viewport drives hydration**. The rows a consumer
declares as its viewport are the rows the detail pass works on. Framework code calls this the
**demand gate**: no row is detailed unless some live view demands it. Everything else stays a
cheap list row — and rows never observed never cost a download.

Recognize it? This is chapter 6's bounded read with the asking automated: there the programmer
chose the window; here the viewport is the window, and it moves with the user.

### The grand total

```rust,no_run
# use vantage_diorama::Dio;
# use vantage_diorama::aggregate::Sum;
# async fn run(dio: Dio) -> vantage_core::Result<()> {
// Grand total of the ROWS column. `rows` is augmented, so each
// computation first hydrates every cached file: the total appears once
// they have all landed, then follows the bucket's changes.
let totals = dio.value_scenery(Sum::of("rows")).open().await?;
# let _ = totals;
# Ok(())
# }
```

A value scenery runs one aggregation (here [`Sum`](vantage_diorama::aggregate::Sum)) over the
rows the Dio admits from its cache. Nothing is asked of the master. It recomputes when the Dio
announces a change, and bumps its generation only when the value actually moved. `open()`
computes the first value before it returns.

Hold on: how can it sum `rows` when most files aren't augmented yet? It doesn't sum the gaps.
The aggregation declares which columns it reads, and when one of them is an augmented column,
the Dio hydrates the rows missing it before the aggregation sees them. A sum that skipped
un-hydrated rows would show a number that looks like a total and isn't one. So the value is
always a total over every cached file, and on a cold cache that means it waits for every file
to download.

That's an expensive number on 122,000 files, and it's meant to be: a total of readings
genuinely needs every file read once. The cost is paid once, because the hydrated columns stay
in `cache.redb`; a warm restart totals from the cache. When you want a cheap number instead,
aggregate something the listing already has.
[`Count::rows()`](vantage_diorama::aggregate::Count) reads no column at all, so it
counts the cached rows without hydrating anything.

(There is a third kind, **RecordScenery**: `dio.record_scenery(id).open()`, one record under the
same contract: read it, subscribe, redraw on bump. A detail pane beside the table would use one;
our table doesn't need it.)

### Open freely, drop when done

Sceneries are inexpensive. A page opens as many as it needs (a grid, a total, a detail pane),
and every `open()` returns a view of its own: its own viewport, its own order, search and
filters. Views of the same query still read one set of rows underneath, so two grids over one
query make one request and hydrate each file once. The other half of that contract: release
them when the page closes. Dropping the handle stops the view's tasks and withdraws its demand,
so a closed page stops pulling data.

### Why does each open get its own view?

Because sharing the view meant sharing its setters. When identical opens returned one shared
instance, a quick-search typed into one grid reshaped every other grid of the same query:
`set_search` on one widget filtered its neighbour's rows too, and two remote clients asking the
same question couldn't scroll to different pages. Sharing was right for the rows and wrong for
everything the user controls.

So the split runs along that line. The rows (the per-query index, the cached records, the
hydration already paid for) are shared between every view of one query. The view (viewport,
shown range, order, search, filters) belongs to whoever opened it. A setter moves only its own
view to the rows of its new query, joining another view's rows when one already answers it.
Chapter 8 leans on this: every HTTP connection opens its own view, and the downloads are still
paid once.

## Binding to a terminal

What remains is rendering — and none of it is specific to this app. Scrolling a virtualized
table, forwarding the visible range as the viewport, redrawing on generation bumps: that's a
reusable binding, and `dataset-ui-adapters` ships it for [ratatui](https://ratatui.rs) in its
`ratatui_dio` module. The entire UI:

```rust,no_run
# use std::sync::Arc;
# use dataset_ui_adapters::ratatui_dio;
# use vantage_aws::prelude::*;
# use vantage_diorama::{TableScenery, ValueScenery};
# async fn ui(
#     scenery: Arc<dyn TableScenery>,
#     totals: Arc<dyn ValueScenery>,
# ) -> vantage_core::Result<()> {
    ratatui_dio::SceneryTable::new(scenery)
        .with_column("FILENAME", "Key", 0)
        .with_column("SIZE", "Size", 10)
        .with_column("ROWS", "rows", 8)
        .with_column("LATEST", "latest", 10)
        .with_status_value("total rows", totals)
        .run()
        .await
        .context("terminal UI failed")
# }
```

Line by line:

- **`ratatui_dio::SceneryTable::new(scenery)`** — hands the scenery to the adapter. The full
  module path marks the boundary: everything before this line was framework, everything after
  is the ratatui binding.
- **`.with_column(header, field, width)`** — one table column: the header text, the record field
  it reads, and a width in characters (`0` = flexible fill). A field the row doesn't have yet
  renders as `…` — which is exactly how un-hydrated rows look.
- **`.with_status_value(label, scenery)`** — pins a `ValueScenery` into the status bar; it
  repaints whenever that scenery's generation bumps, so `total rows` follows the Dio.
- **`.run()`** — takes over the terminal until `q`. It draws only the visible rows (at 122,000
  you don't build widgets for the rest), keeps the scenery's viewport on a **ten-row band around
  the cursor** — details load for the record the user is on and its neighbours, not the whole
  screen. Band-not-screen is the *adapter's* policy, not the scenery's: each detail fetch here
  is a multi-second download, and hydrating all forty visible rows would waste most of that work
  every time the user scrolls on. It listens to every subscribed generation for repaints, and renders the status bar: row
  count, an **augmented** counter (how many rows on hand are fully hydrated), your pinned
  values, and the key legend. `r` calls the scenery's `refresh_source()`: the Dio's reconcile
  on demand, ahead of the timer.

## Running it

`cargo run` on a cold cache is the whole system visible at once. The table appears instantly,
empty, while the view lists the bucket and the background sync copies it; when the sync lands,
the view re-lists from the cache and the row counter passes 122,000. Meanwhile the rows around
the cursor sprout numbers, one file at a time, as the detail pass works through the band (`…`
becoming `14355  20260710`), and the `augmented` counter ticks upward with each one. Move the
cursor, and the band follows; jump to `End`, and the last stations of the alphabet get their
turn. `total rows` stays at its first value, zero over an empty cache, until the total has
every file it needs. Quit, run again: everything already observed is back instantly, warm from
`cache.redb`, and hydration resumes wherever you look next.

Notice what the application never wrote: a render loop, a fetch, an event match. It reads
`scenery.row(idx)` and `totals.value()` through a binding that repaints when a generation
channel says so. Nothing more passes between data and display — and it's precisely how a
real UI binds to Vantage.

---

## What we covered

| Concept                                           | What it does                                                          |
| ------------------------------------------------- | --------------------------------------------------------------------- |
| [`TableScenery`](vantage_diorama::TableScenery)   | Ordered rows by index; sort/search/filter served locally              |
| [`ValueScenery`](vantage_diorama::ValueScenery)   | One aggregation over the Dio's cached rows; hydrates augmented columns it reads |
| [`RecordScenery`](vantage_diorama::RecordScenery) | One record by id, same subscribe/redraw contract                      |
| [`Generation`](vantage_diorama::Generation) / `subscribe()` | Latest-value watch channel — one bump, one repaint          |
| One view per open                                 | Own viewport, order, search and filters; rows shared per query        |
| Two-pass loading                                  | List pass builds the spine; detail pass hydrates rows in view         |
| Whole copy                                        | Once the cache holds the whole set, the list pass reads it, not the master |
| `set_viewport(loaded, shown)`                     | Declares what loads, and thereby what hydrates; `shown` is what's on screen |
| `set_order` / `set_search` / `set_filters`        | Reshape one view in place, without reopening                          |
| `Loader::eager(..).blocking(false)`               | UI first: `build()` returns immediately, the sync runs behind it      |
| `Refresher::new(..).every(..)`                    | Standing freshness: reconcile on a timer; `refresh_source()` on demand |
| `ratatui_dio::SceneryTable`                       | The ratatui binding: virtualized rows, viewport, status bar, keys     |

```admonish tip title="What's next"
One terminal, one viewport. A web server is the same picture multiplied: every connected
browser is its own standing view, each on a different page, all expecting details to stream in
— and none of them should ever download a file another view already paid for. The final chapter
puts this Dio behind chapter 3's Axum server: kubernetes-style GET + watch endpoints, a React
frontend, and a scheduler that serves every concurrent viewer fairly.
```
