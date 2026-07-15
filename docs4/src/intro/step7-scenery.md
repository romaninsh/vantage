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
| **How many**  | One per data segment                             | Many per Dio — one per view variant, shared when identical |
| **Lifecycle** | Long-lived; owns cache and background tasks      | Opened by a view, dropped with it                      |
| **Consumer**  | Lens callbacks, handlers, scripts                | UI widgets, bound through an *adapter*                 |

That last cell is where this chapter ends up. The consumer — a terminal table here, a desktop
grid or a remote client such as a React page elsewhere — drives its scenery in one repeating
loop:

1. The consumer declares what it shows: `set_viewport(40..80)` — nothing more than "these rows
   are on screen."
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
- **standing freshness** — the bucket changes, the table follows, plus a running total that
  ticks upward as data lands.

Measure chapter 6's ending against that. It read from the Dio with
`dio.vista().fetch_window(0, 10)` — and it works fine, for a CLI: the read is bounded to ten
records, and it resolves once all ten have their details. Nothing blocks — it's an async wait,
the runtime stays free — but the *user* waits all the same: twenty seconds of silence on a cold
cache, and every widening of the window widens the wait. For a better experience we need a more
responsive UI:

1. display results as soon as we have them — don't hold rows hostage to their details;
2. show data immediately and fix it reactively as better values arrive;
3. register what the user is currently looking at, reducing what is fetched and transmitted.

The Scenery implements exactly this:

- rows are served the moment it opens — whatever the cache already holds, with `…` standing in
  for details still pending;
- every landed detail updates its own row and bumps the `Generation` — one tick, one repaint;
- `set_viewport(range)` registers the user's attention, and only those rows hydrate.

For the consumer, this chapter reaches for **ratatui** — a terminal-UI library — which Vantage
updates in real time through a ready-made adapter. The `dataset-ui-adapters` crate ships such
adapters for several UI frameworks (egui, Slint, GPUI, Cursive, Tauri among them), and they all
speak the same scenery loop. Here is ours a few seconds into a cold start — the listing still
streaming in and the first two visible rows already hydrated:

```text
FILENAME                              SIZE        ROWS      LATEST
csv/by_station/ACW00011604.csv        41673       1231      19490814
csv/by_station/ACW00011647.csv        481021      14355     20260710
csv/by_station/AE000041196.csv        1998959     …         …
…
 8000 rows · 2 augmented · total rows 15586 · ↑/↓ PgUp/PgDn scroll · r refresh · q quit
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
        // S3's per-response maximum — we're listing the whole station set.
        .with_condition(eq("max-keys", 1000))
```

---

## A Lens that serves a UI

Chapter 5's Lens had one job: warm the cache before anyone reads. A UI inverts the priorities —
the screen must appear *immediately*, on whatever is known so far, and data streams in behind
it. Three additions to the builder:

```rust
let lens = Arc::new(
    Lens::new()
        .cache_at("cache.redb")
        .on_start(|dio| {
            let dio = dio.clone();
            async move { sync(&dio).await }
        })
        // Don't wait for the sync: the UI opens on whatever the cache
        // holds (nothing, on a first run) and rows stream in behind it.
        .on_start_blocking(false)
        // The scenery's list pages come straight from the warmed cache —
        // zero network. The master is only contacted by the refresh
        // reconcile and the per-row detail fetches.
        .on_list_page(|dio, q| {
            let dio = dio.clone();
            async move {
                Ok(dio
                    .cache()
                    .list_values()
                    .await?
                    .into_iter()
                    .skip(q.offset)
                    .take(q.limit)
                    .collect())
            }
        })
        // Reconcile against the bucket once a minute: new files appear
        // as un-hydrated rows, vanished ones drop out, changed ones are
        // demoted for re-hydration.
        .refresh_every(Duration::from_secs(60))
        .build()
        .context("Failed to build lens")?,
);
```

**`on_start_blocking(false)`** detaches the sync: `make_dio` returns at once and `on_start` runs
as a background task. The UI's first frame shows an empty table; a few minutes later it shows
122,000 rows — filling in page by page in between, a thousand at a time. For that to be visible,
the sync itself gains one line — after each landed page it announces the change on the event
bus, and every open view re-reads:

```rust
        dio.cache().insert_values(page.into_iter().collect()).await?;
        dio.notify_dataset_changed();
```

(The `println!` narration from chapter 5 is gone — a terminal UI owns the screen now.)

**`on_list_page`** needs the most context, because it names how a `TableScenery` loads. An
augmented Dio drives **two-pass loading**: a *list pass* fetches cheap rows and establishes the
view's order — its spine — and a *detail pass* hydrates individual rows' augment columns as the
viewport reaches them. The passes are separately pluggable. The detail pass is already defined —
it's chapter 6's augmentation. The list pass, left alone, would ask the master — a full
122-request S3 walk every time a view (re)builds its spine. But chapter 5 built something better:
a cache that *is* the listing. Registering `on_list_page` overrides the list pass; the
[`QueryDescriptor`](vantage_diorama::QueryDescriptor) argument carries the window (`offset`,
`limit` — plus conditions, sort, and search when the view has them), and our implementation is a
window over the cache. The scenery's spine now costs zero network.

**`refresh_every`** completes the freshness story. Once a minute the Dio reconciles its cache
against the master (this *does* walk the listing — 122 requests, in the background): new files
appear as un-hydrated rows, deleted ones drop out, and a file whose listing entry changed keeps
showing its old numbers but is marked stale — the next time it's on screen, the detail pass
re-fetches it. Chapter 5 needed `--invalidate` to notice deletions; a standing view just notices.

## Creating the reactive UI

Our UI needs two things from the Dio: the rows, and a running total. Each one is a scenery.
Let's open them one at a time.

### The table

```rust
// One list page covers the whole cached listing (~122k station files);
// the viewport's detail pass hydrates whatever is on screen first.
let scenery = dio.table_scenery().page_size(200_000).open().await?;
```

`.open()` seeds the scenery from the cache, spawns its reactor (the task that watches the Dio's
event bus), and hands back `Arc<dyn TableScenery>`. What you get is deliberately small and
synchronous:

- `row_count()` and `row(idx)` — an [`EnrichedRecord`](vantage_diorama::EnrichedRecord): the
  record plus a per-row status;
- `set_viewport(range)` — more on this in a moment;
- `subscribe()` — the generation channel from the start of the chapter.

The builder can also chain `sort(col, dir)`, `search(text)`, and `where_eq(col, value)` — every
one of them served locally, on a backend that can do none of it. We don't use them here — the
table shows the archive as-is.

A UI rarely commits to one order at open time, though — and it doesn't have to reopen. The
handle mutates in place: `set_sort(col, dir)` and `set_search(text)`, bound to a key or a header
click, re-point the scenery at the ordered index for the new variant, swap the visible rows in
one atomic step (the grid never blanks mid-reorder), and restart hydration for whatever is on
screen. Sorting back reuses the already-built index — zero list calls. Conditions are the
exception: `where_eq` defines what the view *is*, so it's set at open, not toggled on a live
scenery.

One thing probably caught your eye: `page_size(200_000)`?! That's the *list pass* — how many
spine rows one `on_list_page` call returns. It's a separate axis from the viewport, which drives
only the *detail pass* over rows the spine already holds. Left at the default, the spine would
stop at the first hundred files and nothing in this UI asks for page two. Set past the archive
size, the first list call builds the entire spine — one cheap read of the local cache.

### The viewport

The **viewport** is the scenery's answer to "which rows is the user actually looking at?" — a
plain range of row indexes, like `40..80`. It's declared through `set_viewport(range)`, the
method on the handle we just opened. You rarely call it yourself: whoever renders the table
calls it as the user scrolls — later in this chapter, that's the ratatui adapter's job.

It is the load-bearing call, because **the viewport drives hydration**. The rows a consumer
declares as its viewport are the rows the detail pass works on. Framework code calls this the
**demand gate**: no row is detailed unless some live view demands it. Everything else stays a
cheap list row — and rows never observed never cost a download.

Recognize it? This is chapter 6's bounded read with the asking automated: there the programmer
chose the window; here the viewport is the window, and it moves with the user.

### The running total

```rust
// Grand total of the ROWS column — recomputes reactively as files
// hydrate, so the status bar counts up while data arrives.
let totals = dio.value_scenery().sum("rows").open().await?;
```

Hold on — how can it sum `rows` when most rows aren't augmented yet? It doesn't wait for them:

- it recomputes over the *cache* whenever the Dio announces a change;
- rows that have no `rows` column yet are skipped — the sum covers what has been observed so far;
- each detail fetch that merges `rows` into the cache fires `RecordChanged`; the scenery
  recomputes and bumps its generation only when the value actually moved.

So the number in the status bar is honest about coverage: it starts at zero and climbs with
hydration, one landed file at a time.

(There is a third kind, **RecordScenery** — `dio.record_scenery(id)` — one record under the same
contract: read it, subscribe, redraw on bump. A detail pane beside the table would use one; our
table doesn't need it.)

### Open freely, drop when done

Sceneries are inexpensive. A page opens as many as it needs — a grid, a running total, a detail
pane — and identical opens share one instance under the hood (the sharing key is the scenery's
query — sort, search, conditions — plus the columns it demands; two views asking the same
question get the same scenery). The other half of that contract: release them when the page
closes. Dropping the handle stops the scenery's tasks and withdraws its demand, so a closed
page stops pulling data. Chapter 8 meets the one case where sharing is wrong — two remote
viewers asking the same question but scrolling different pages — and opts out per scenery.

## Binding to a terminal

What remains is rendering — and none of it is specific to this app. Scrolling a virtualized
table, forwarding the visible range as the viewport, redrawing on generation bumps: that's a
reusable binding, and `dataset-ui-adapters` ships it for [ratatui](https://ratatui.rs) in its
`ratatui_dio` module. The entire UI:

```rust
    ratatui_dio::SceneryTable::new(scenery)
        .with_column("FILENAME", "Key", 0)
        .with_column("SIZE", "Size", 10)
        .with_column("ROWS", "rows", 8)
        .with_column("LATEST", "latest", 10)
        .with_status_value("total rows", totals)
        .run()
        .await
        .context("terminal UI failed")
```

Line by line:

- **`ratatui_dio::SceneryTable::new(scenery)`** — hands the scenery to the adapter. The full
  module path marks the boundary: everything before this line was framework, everything after
  is the ratatui binding.
- **`.with_column(header, field, width)`** — one table column: the header text, the record field
  it reads, and a width in characters (`0` = flexible fill). A field the row doesn't have yet
  renders as `…` — which is exactly how un-hydrated rows look.
- **`.with_status_value(label, scenery)`** — pins a `ValueScenery` into the status bar; it
  repaints whenever that scenery's generation bumps, so our `total rows` sum ticks live.
- **`.run()`** — takes over the terminal until `q`. It draws only the visible rows (at 122,000
  you don't build widgets for the rest), keeps the scenery's viewport on a **ten-row band around
  the cursor** — details load for the record the user is on and its neighbours, not the whole
  screen. Band-not-screen is the *adapter's* policy, not the scenery's: each detail fetch here
  is a multi-second download, and hydrating all forty visible rows would waste most of that work
  every time the user scrolls on. It listens to every subscribed generation for repaints, and renders the status bar: row
  count, an **augmented** counter (how many rows on hand are fully hydrated), your pinned
  values, and the key legend. `r` runs the Dio's reconcile on demand, ahead of the timer.

## Running it

`cargo run` on a cold cache is the whole system visible at once. The table appears instantly —
empty. Within a second the first thousand filenames arrive; the row counter keeps climbing as
the background sync streams pages, passing 122,000 a few minutes in. Meanwhile the rows around
the cursor sprout numbers, one file at a time, as the detail pass works through the band — `…`
becoming `14355  20260710` — and the `augmented` counter and `total rows` sum tick upward with
each one. Move the cursor, and the band follows; jump to `End`, and the last stations of the
alphabet get their turn. Quit, run again: everything already observed is back instantly, warm
from `cache.redb`, and hydration resumes wherever you look next.

Notice what the application never wrote: a render loop, a fetch, an event match. It reads
`scenery.row(idx)` and `totals.value()` through a binding that repaints when a generation
channel says so. Nothing more passes between data and display — and it's precisely how a
real UI binds to Vantage.

---

## What we covered

| Concept                                           | What it does                                                          |
| ------------------------------------------------- | --------------------------------------------------------------------- |
| [`TableScenery`](vantage_diorama::TableScenery)   | Ordered rows by index; sort/search/filter served locally              |
| [`ValueScenery`](vantage_diorama::ValueScenery)   | Reactive aggregate — `count`, `sum`, `max`, `min`, or `custom`        |
| [`RecordScenery`](vantage_diorama::RecordScenery) | One record by id, same subscribe/redraw contract                      |
| [`Generation`](vantage_diorama::Generation) / `subscribe()` | Latest-value watch channel — one bump, one repaint          |
| Two-pass loading                                  | List pass builds the spine; detail pass hydrates rows in view         |
| `on_list_page`                                    | Plug the list pass — ours serves pages from the chapter-5 cache       |
| `set_viewport(range)`                             | Declares what's visible — and thereby what hydrates                   |
| `on_start_blocking(false)`                        | UI first: `make_dio` returns immediately, the sync streams behind it  |
| `refresh_every` + `notify_dataset_changed`        | Standing freshness: reconcile on a timer, announce every change       |
| `ratatui_dio::SceneryTable`                       | The ratatui binding: virtualized rows, viewport, status bar, keys     |

```admonish tip title="What's next"
One terminal, one viewport. A web server is the same picture multiplied: every connected
browser is its own standing view, each on a different page, all expecting details to stream in
— and none of them should ever download a file another view already paid for. The final chapter
puts this Dio behind chapter 3's Axum server: kubernetes-style GET + watch endpoints, a React
frontend, and a scheduler that serves every concurrent viewer fairly.
```
