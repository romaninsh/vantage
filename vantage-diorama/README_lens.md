# Configuring a Lens

A Lens is the policy every Dio built from it shares: where rows are cached,
how the cache is filled, how often it is re-read, and where writes go. An
application usually has one, and a second only when two parts of it want
genuinely different answers to those questions.

The policy lives in the Lens, not in each Dio, because the same questions
come up for every table. Answered once, a new table costs one line,
`lens.dio(vista).build().await?`, and behaves like the others.

## The four settings

```rust
use std::time::Duration;

use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, FlashOutcome, Lens, Loader, LoaderCx, Refresher};

let lens = Lens::builder()
    .cache(Cache::redb("cache.redb"))
    .loader(Loader::eager(reload))
    .refresher(Refresher::new(reload).every(Duration::from_secs(300)))
    .writer(|cx: LoaderCx, flash| async move {
        send(cx.master(), &flash).await?;
        Ok(FlashOutcome::applied())
    })
    .build()?;

let dio = lens.dio(orders_vista).build().await?;

async fn reload(cx: LoaderCx) -> vantage_core::Result<()> {
    let rows = cx.master().list_values().await?;
    cx.reconcile(rows).await
}
```

| Setting | Required | Without it |
|---|---|---|
| `cache(Cache)` | yes | `build` fails: a Dio has nowhere to keep rows |
| `loader(Loader)` | no | the cache is all there is; nothing fills it |
| `refresher(Refresher)` | no | no timer; `dio.refresh()` re-reads the set the way it was loaded |
| `writer(..)` | no | writes go to the master, gated by its capabilities |

Two more settings tune rather than decide: `tuning(Tuning)` holds the
per-Dio knobs (see [Tuning](#tuning)), and `runtime(Handle)` names the tokio
runtime the Lens spawns on. Without one, `build` takes the current runtime
and fails outside one.

`build` returns `Result<Arc<Lens>>`. Every problem it finds is an error that
names the setting (`setting = "loader.blocking"`), and it never panics. The
`Arc` is there because each Dio holds its Lens alive: the loader, refresher
and writer run on the Dio's tasks long after the line that built them.

Each callback receives a `LoaderCx`, an owned, cheap-to-clone handle on the
Dio it serves. It has three things on it:

- `cx.master()`: the master Vista, narrowed for this Dio.
- `cx.cache()`: the Dio's cache table. Writes here raise no events and ignore
  flashes in flight, so use it for bulk copies whose arrival is announced
  some other way.
- `cx.reconcile(rows)`: make the cache hold exactly `rows`, the whole set.
  Rows with a flash in flight, or whose flash settled since the callback
  began, are left alone; the refresh time is recorded and one
  `DatasetChanged` is published.

`cx.fetch(window, &query)` reads one window of the master narrowed by a
view's query. A paged loader is its main user (see
[README_datasource.md](README_datasource.md)).

### Why are callbacks handed an owned context?

Each callback returns a `'static` future, so it can't borrow anything from
the call. The `LoaderCx` is owned and the future holds it across awaits.
State the callback needs (a queue, a metrics client) is captured by `move`
and cloned into each call, as the outbox example below shows.

## Cache

`Cache` picks the store every Dio of the Lens keeps its rows in:

- `Cache::redb(path)`: a redb file, created if absent and opened when the
  Lens is built. It survives restarts, so a cold launch paints from disk.
- `Cache::memory()`: process-local, nothing survives. Servers, tests,
  short-lived processes.
- `Cache::backend(Arc<dyn CacheBackend>)`: any other store, or one store you
  opened yourself and want to share.

```rust
use std::sync::Arc;

use vantage_diorama::{Cache, Lens, RedbCache};

let store = Arc::new(RedbCache::open("cache.redb")?);
let editing = Lens::builder().cache(Cache::backend(store.clone())).build()?;
let reporting = Lens::builder().cache(Cache::backend(store)).build()?;
```

### Cache identity

Each Dio claims one table in the store. The table's name is the Dio's cache
identity: the vista's name when nothing narrows or orders it, otherwise
`name?key`, where `key` is the applied query (conditions, search and order).
A vista narrowed before `lens.dio(..)` gets a table of its own, and so does
every `dio.derive()` with a different query. Two Dios share a table only when
their queries are the same.

Control characters, `%`, and a `?` in the vista's name are percent-encoded,
so the first `?` always separates the name from the key, and no vista can
pass for another vista's narrowed table.

The identity holds no Lens, no loader and no augmentation. That has two
consequences worth knowing:

- Two Lenses over one backend (the `editing` and `reporting` pair above)
  read and write the same table for the same vista. That is usually the
  point of sharing the store. When it isn't, give each Lens its own store.
- A plain Dio and an augmented Dio over one vista are two Dios on one cache
  table. Once the augmented one hydrates its rows, the plain one reads the
  merged columns too. Whether augmentation should join the identity is an
  open question; until it is settled, keep the two on separate stores if
  the plain Dio must not see merged columns.

### The Lens registry

A Lens remembers every Dio it built that something still holds. A second
build with the same cache identity, the same augmentations and the same
`live` setting returns the live Dio instead of starting another: one eager
start, one write queue, one set of sceneries. The shared build still checks
the cache against the refresher's `max_age`.

Augmentations are compared by the `Arc` that `.augment(..)` wraps them in, so
two separate `.augment(vec![..])` calls build two Dios. Derivations pass
their parent's `Arc` on, so two derivations of one query share. Once every
holder drops a Dio, the next build makes a new one with its own eager start;
a dropped Dio is never revived.

## Loader

The loader fills a Dio's cache from its master. There are three shapes.

**Eager** copies the whole set once, when the Dio is built. Sceneries then
read the cache, and every clause a view adds is evaluated over it:

```rust
let lens = Lens::builder()
    .cache(Cache::memory())
    .loader(Loader::eager(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    }))
    .build()?;
```

A large set that the master serves by cursor can be copied page by page,
resuming from the last cached key:

```rust
async fn sync(cx: &LoaderCx) -> VantageResult<()> {
    let mut token: Option<CborValue> = cx
        .cache()
        .list_values()
        .await?
        .keys()
        .last()
        .cloned()
        .map(Into::into);
    loop {
        let (page, next) = cx.master().fetch_cursor(token, PAGE_SIZE).await?;
        cx.cache().insert_values(page.rows.into_iter().collect()).await?;
        if next.is_none() {
            return Ok(());
        }
        token = next;
    }
}

let lens = Lens::builder()
    .cache(Cache::redb("cache.redb"))
    .loader(Loader::eager(|cx: LoaderCx| async move { sync(&cx).await }).blocking(false))
    .build()?;
```

This one writes `cx.cache()` directly, so it raises no event per page. The
Dio announces the whole set with `DioEvent::Seeded` once the start returns.

**Paged** fetches the windows a table scenery asks for, as it asks for them.
A million-row table never lands in the cache whole. **Paged with an eager
fallback** pages a master that advertises `Window` and copies one that
doesn't, so one Lens serves both kinds of table. Both are covered with the
data-source side in [README_datasource.md](README_datasource.md).

### Why does the eager start block by default?

A grid that mounts at launch over an empty cache shows an empty table until
the copy lands. A short wait inside `build` is the better experience on a
desktop, so `build` waits by default. `.blocking(false)` returns at once and
lets the copy land in the background: the right call for a server that must
not stall startup on a slow upstream, and for a copy that takes minutes.
`blocking` applies only to an eager start; on a paged loader without one,
`build` fails.

## Refresher

The refresher re-reads a Dio's master into its cache. It runs on
`dio.refresh()` and, with `every`, on a timer:

```rust
let lens = Lens::builder()
    .cache(Cache::memory())
    .loader(Loader::eager(reload))
    .refresher(Refresher::new(reload).every(Duration::from_secs(1)))
    .build()?;
```

`max_age(ttl)` bounds how stale a cache may be when it is opened. A Dio whose
last refresh is older than `ttl`, or was never recorded, gets a background
refresh when it is built and whenever a table or record scenery opens on it.
The refresh time is persisted in the cache, so on a redb store it holds
across restarts. `max_age` works without `every`: a server that wants
"never older than a minute when someone looks" and no polling sets only
`max_age`.

A refresher that calls `cx.reconcile` has already recorded the refresh and
published `DatasetChanged`. One that writes `cx.cache()` directly gets both
done for it when it returns. A failed timed refresh is logged and the cache
keeps serving what it had.

A refresher is also where cross-cutting work fits. This one times every
re-read:

```rust
let metrics = Arc::new(Metrics::default());

let refresher = Refresher::new({
    let metrics = metrics.clone();
    move |cx: LoaderCx| {
        let metrics = metrics.clone();
        async move {
            let started = Instant::now();
            let rows = cx.master().list_values().await?;
            metrics.record(cx.master().name(), started.elapsed(), rows.len());
            cx.reconcile(rows).await
        }
    }
})
.every(Duration::from_secs(60));
```

## Writer

Without a writer, a flash goes to the master, and the Dio is only as
writable as the master says it is. A writer takes every write instead and
makes every Dio of the Lens writable, whatever its master allows. A
read-only CSV becomes editable, with the changes landing wherever the writer
sends them.

```rust
let lens = Lens::builder()
    .cache(Cache::redb("cache.redb"))
    .writer(|cx: LoaderCx, flash: ChangeFlash| async move {
        let master = cx.master();
        match flash.kind() {
            FlashKind::Insert => {
                let id = flash.id().expect("insert has id").to_string();
                master.insert_value(id, flash.patch()).await?;
            }
            FlashKind::Patch => {
                let id = flash.id().expect("patch has id").to_string();
                master.patch_value(id, flash.patch()).await?;
            }
            FlashKind::Delete => {
                let id = flash.id().expect("delete has id").to_string();
                master.delete(id).await?;
            }
            other => {
                return Err(error!(
                    "the writer has no route for this flash kind",
                    kind = format!("{other:?}")
                ));
            }
        }
        Ok(FlashOutcome::applied())
    })
    .build()?;
```

The Dio has already staged the change in the cache when the writer runs, so
every scenery shows it at once. `Ok(FlashOutcome)` confirms it:
`FlashOutcome::applied()` keeps the staged value, and `.with_id(id)` moves
the row to the id the backend chose, stored as `.with_record(row)` when the
backend reported one. `Err`, or a panic, rolls the row back and publishes
`WriteReverted`.

A Dio runs its writes one at a time, in order. So the writer must not write
through the Dio it serves: a flash on that Dio from inside the writer
returns an error naming the Dio rather than waiting on itself. Write to the
master, to another Dio, or to `cx.cache()`.

### Offline edits through an outbox

A field app has to accept edits with no signal. The writer confirms each
flash locally and queues it; the refresher drains the queue when the master
is reachable, then reconciles:

```rust
let outbox: Arc<Mutex<Vec<ChangeFlash>>> = Arc::default();

let lens = Lens::builder()
    .cache(Cache::redb("field-app.redb"))
    .writer({
        let outbox = outbox.clone();
        move |_cx: LoaderCx, flash: ChangeFlash| {
            let outbox = outbox.clone();
            async move {
                outbox.lock().unwrap().push(flash);
                Ok(FlashOutcome::applied())
            }
        }
    })
    .refresher(
        Refresher::new({
            let outbox = outbox.clone();
            move |cx: LoaderCx| {
                let outbox = outbox.clone();
                async move {
                    loop {
                        let Some(flash) = outbox.lock().unwrap().first().cloned() else {
                            break;
                        };
                        send(cx.master(), &flash).await?;
                        outbox.lock().unwrap().remove(0);
                    }
                    let rows = cx.master().list_values().await?;
                    cx.reconcile(rows).await
                }
            }
        })
        .every(Duration::from_secs(30)),
    )
    .build()?;
```

The order matters. Reconcile makes the cache match the master, so
reconciling while an edit is still queued would revert it on screen. Here a
failed send returns before the reconcile, and the queue keeps the flash for
the next tick. The queue in this snippet lives in memory to show the shape;
a real outbox persists, and decides what a conflict means for its domain.

### Why not send the edit from inside the writer and wait for signal?

Because the writer's return is the confirmation, and the Dio's writes run
one at a time. A writer that waits for the network holds every later write
of that Dio behind it: the user's next edit shows, but stays pending until
the device finds coverage. Confirm locally, return, and let the refresher do the slow
part, as the outbox does. The flash carries what a later sender needs:
`flash.before()` is the pre-image and `flash.after()` the merged row.

## Live follow

A master that advertises `Subscribe` can push its changes. `.live()` on the
Dio builder follows that stream:

```rust
let dio = lens.dio(master).live().build().await?;
```

The subscription is made before the eager start copies the set, and the
changes that arrive meanwhile are applied once the copy lands, so none is
lost or overwritten. A follow that falls behind re-reads the set. Over a
master without `Subscribe`, `.live()` does nothing and the refresher keeps
the cache fresh.

A source that pushes through something the master doesn't model (a webhook,
a message queue) feeds the Dio directly:

```rust
let feed = dio.clone();
tokio::spawn(async move {
    while let Some(change) = rx.recv().await {
        if let Err(e) = feed.apply_change(change).await {
            eprintln!("change failed to apply: {e}");
        }
    }
});
```

`apply_change` writes the row and publishes the matching event;
`ChangeEvent::Invalidated` runs a full refresh.

### Why keep a refresher if the Dio follows live?

Push channels drop messages: a connection blips, a server restarts, a laptop
sleeps. A follow that notices it fell behind recovers by itself, but one that
never learns about a lost message can't. A slow `every` on the refresher is
the safety net that makes the cache correct eventually whatever the stream
did.

## Events

Every Dio has an event bus. Sceneries subscribe to it; so can anything else:

```rust
let mut bus = dio.subscribe_events();
tokio::spawn(async move {
    while let Ok(event) = bus.recv().await {
        match event {
            DioEvent::RefreshFailed { error } => eprintln!("live re-read failed: {error}"),
            DioEvent::WriteReverted { id, error, .. } => eprintln!("{id} reverted: {error}"),
            DioEvent::LoadFailed { range, error } => eprintln!("{range:?} failed: {error}"),
            _ => {}
        }
    }
});
```

`DioEvent` is `#[non_exhaustive]`, so every match needs a fallback arm. The
ones a policy is likely to care about:

- `DatasetChanged`: the set moved; views re-read. `reconcile` and every
  successful refresh publish it.
- `Seeded`: an eager start finished writing the whole set to the cache.
- `RefreshFailed { error }`: a live follow failed to re-read the set after a
  dropped subscription or a change that wouldn't apply. The follow retries
  with backoff, and views keep the rows they show.
- `WritePending`, `WriteReverted`, `WriteFailed`: a flash was staged, failed
  and rolled back, or failed with no row to stage.
- `LoadFailed { range, error }`: a paged load failed, and the slots in
  `range` keep what they had.

`LoadFailed` has a second sender. A view that can't answer its query (the
master rejected a clause, or refused one and there is no complete set to
evaluate it over) withholds its rows and publishes `LoadFailed` with its
viewport, or `0..0` before it has one. Nothing was fetched: this is a view
failure, not a failed load. The bus can't tell the two apart, but the view
can. Its `load_state()` is `LoadState::Failed(failure)`, naming the table and
the clause, until the query changes.

## Tuning

`Tuning` holds the knobs every Dio of the Lens shares. Start from
`Tuning::default()` and change fields with the `with_*` setters:

| Field | Default | What it trades |
|---|---|---|
| `write_queue` | 256 | Writes past it wait for room before staging. A full queue is overload; surface it rather than hide it behind a bigger buffer. |
| `viewport_debounce` | 50ms | Rapid viewport moves coalesce into one fetch. Wider absorbs scroll bursts over a slow upstream; narrower feels snappier. |
| `augment_workers` | 1 | Concurrent per-row detail fetches. One keeps the order deterministic; raise it when the detail source tolerates parallel requests. |
| `refresh_on_open` | true | A paged view re-fetches its first window on open, over what the cache showed. Turn off for offline or read-only use. |
| `aggregate_debounce` | 250ms | Cooldown between aggregate recomputes. The first change acts at once; the rest of a burst is flushed at the end. |

```rust
let lens = Lens::builder()
    .cache(Cache::memory())
    .tuning(Tuning::default().with_write_queue(4096).with_refresh_on_open(false))
    .build()?;
```

`write_queue` and `augment_workers` must be non-zero; `build` says so if
they aren't.

## Lens checklist

A Lens that is ready for production has:

1. **A cache** that matches the process: redb where a cold start should
   paint from disk, memory where nothing should outlive the process, and a
   shared backend only where Lenses should see each other's tables.
2. **A loader** that matches the master: eager for sets that fit, paged for
   sets that don't, paged with `or_eager` when one Lens serves both.
3. **A refresher** with `every` at the interval where staleness becomes
   visible, or `max_age` alone where nobody should poll.
4. **A writer** only when writes must go somewhere other than the master,
   and never one that flashes its own Dio.
5. **Event handling** for `RefreshFailed` and `WriteReverted`, and views that
   read `load_state()` to tell a view failure from a failed load.
