# Dio & Lens — Caching and Events

Chapter 4 gave you Vista — a universal handle that works with any backend. But every call still hits
the backend. That was easy to forgive with a local SQLite file; this chapter picks a data source
where it hurts: a real cloud API, hundreds of milliseconds away, read-only, and unable to sort or
search. We'll build a small CLI around it — an inventory of a public S3 bucket, with a cache.

**Diorama** (`vantage-diorama`) is the layer that makes this pleasant. It sits between your Vista
and whatever consumes it, and does three things:

1. **Transparent caching.** Keep a local copy of your data. Reads come from cache, not from the
   master — a listing that costs seconds over the network costs microseconds after the first fetch.
2. **Capability injection.** A Vista backed by S3 can't sort, search, or paginate a listing.
   Diorama caches the dataset locally and answers those queries from cache — the consumer sees a
   richer handle than the backend actually offers.
3. **Custom write routing.** Writes don't have to go to the master — useful when the master can't
   take them at all. You decide what a write means: a queue, a different store, an API call.

```admonish question title="Why 'Diorama'?"
The vista from the peak is magnificent — and far away. Sooner or later you want a piece of it
close at hand: on your desk, under glass, alive. A **diorama** is exactly that — a crafted
miniature of a real scene, small enough to hold, faithful enough to study.

That's the layer this chapter builds. A **Lens** is the optics you capture the scene through —
ground once, reused for every capture, deciding what is kept and how it refreshes. A **Dio** is
one captured segment: your local, living copy of the data, reconciling with the world it depicts.
And once the miniature is lit and running, **Scenery** (chapter 7) is what an audience actually
watches.
```

---

## Table, Vista, Dio

This is the third handle to the same records — and like the query-vs-table comparison back in
chapter 2, each one trades something away for something new:

|                  | `Table<DB, E>`                  | `Vista`                          | `Dio`                                     |
| ---------------- | ------------------------------- | -------------------------------- | ------------------------------------------ |
| **Purpose**      | Model your data; business logic | Let generic code consume any table | Keep a live local copy of a data segment |
| **Typing**       | Compile-time entity & backend   | Schema carried at runtime        | Same records as Vista                      |
| **Reads**        | Query the backend               | Query the backend                | Served from a local cache                  |
| **Writes**       | Applied immediately             | Applied immediately              | Enqueued — routed by policy                |
| **Changes**      | You re-query                    | You re-query                     | Announced on an event bus                  |
| **Capabilities** | Whatever the backend supports   | Honestly advertised              | Extended — the cache fills the gaps        |
| **Mode**         | Transactional                   | Transactional                    | Live                                       |
| **Lifecycle**    | A definition — cheap to clone, narrowed per use | A handle — built, narrowed, dropped | Long-lived — owns its cache, queue, and background tasks |

### Caching

Diorama caches at two levels. **Page segments** hold windows of an ordered query result — they're
what lets a viewport scroll a huge listing without re-asking the master, and they power the
loading strategies you'll meet in chapter 7. Beneath them sits the **key/value record store** —
one entry per record id, plain and dumb. We keep it simple for now: everything in this chapter
runs on the record store alone.

<svg viewBox="0 0 760 240" xmlns="http://www.w3.org/2000/svg" font-family="sans-serif" font-size="13">
  <defs>
    <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="#888"/>
    </marker>
  </defs>

  <!-- Dio container -->
  <rect x="20" y="30" width="460" height="180" rx="12" fill="none" stroke="#8f5a2d" stroke-width="2.5"/>
  <text x="250" y="56" text-anchor="middle" fill="#8f5a2d" font-weight="bold" font-size="15">Dio</text>

  <!-- master -->
  <rect x="45" y="75" width="170" height="90" rx="8" fill="#4a7c59"/>
  <text x="130" y="105" text-anchor="middle" fill="#fff" font-weight="bold">master</text>
  <text x="130" y="125" text-anchor="middle" fill="#fff" font-size="12">Vista — S3 listing</text>
  <text x="130" y="145" text-anchor="middle" fill="#fff" font-size="12" fill-opacity="0.8">~200 ms per page</text>

  <!-- cache -->
  <rect x="285" y="75" width="170" height="90" rx="8" fill="#4a7c59"/>
  <text x="370" y="105" text-anchor="middle" fill="#fff" font-weight="bold">cache</text>
  <text x="370" y="125" text-anchor="middle" fill="#fff" font-size="12">redb — id → record</text>
  <text x="370" y="145" text-anchor="middle" fill="#fff" font-size="12" fill-opacity="0.8">µs away</text>

  <!-- pump arrow -->
  <line x1="215" y1="120" x2="281" y2="120" stroke="#888" stroke-width="2" marker-end="url(#arrow)"/>
  <text x="250" y="190" text-anchor="middle" fill="#888" font-size="12">the Lens pumps master → cache</text>

  <!-- Consumers -->
  <rect x="580" y="45" width="160" height="58" rx="10" fill="#7c2d8f"/>
  <text x="660" y="69" text-anchor="middle" fill="#fff" font-weight="bold">facade Vista</text>
  <text x="660" y="88" text-anchor="middle" fill="#fff" font-size="12">dio.vista()</text>

  <rect x="580" y="140" width="160" height="58" rx="10" fill="#7c2d8f" fill-opacity="0.55"/>
  <text x="660" y="164" text-anchor="middle" fill="#fff" font-weight="bold">Sceneries</text>
  <text x="660" y="183" text-anchor="middle" fill="#fff" font-size="12">chapter 7</text>

  <!-- read arrows -->
  <line x1="484" y1="74" x2="576" y2="74" stroke="#888" stroke-width="2" marker-end="url(#arrow)"/>
  <line x1="484" y1="169" x2="576" y2="169" stroke="#888" stroke-width="2" marker-end="url(#arrow)"/>
  <text x="530" y="128" text-anchor="middle" fill="#888" font-size="12">reads — µs</text>
</svg>

A **Dio** owns exactly the two blocks above: a *master* Vista — the source of truth, however far
away — and a *cache*, the local copy. What it deliberately does **not** decide is policy: when the
cache fills, when it goes stale, what a write means. There are too many valid answers for one
default — seed once and keep forever, re-fetch on a timer, reconcile from a push stream, write
through to the master, write somewhere else entirely. That's what the **Lens** is for: it
describes how the cache is used. The simplest possible Lens pumps the master into the cache once,
when the Dio loads:

```rust
let lens = Arc::new(
    Lens::new()
        .cache_at("cache.redb")
        .on_start(|dio| {
            let dio = dio.clone();
            async move {
                // Pump: read everything from the master, write it to the cache.
                let rows = dio.master().list_values().await?;
                dio.cache().insert_values(rows).await
            }
        })
        .build()?,
);

let dio = lens.make_dio(vista).await?;   // runs on_start, returns a warm Dio
```

With the Dio in place, there are two ways to consume it. The first is the **facade Vista** —
`dio.vista()` — for *proactive* querying: you ask, it answers, exactly like the backend's own
Vista from chapter 4. Same interface, same records — except the answers come from the cache, and
the capability set can be *wider* than the master's, because the Lens decides what to add: a
read-only master gains writes when the Lens routes them somewhere, and counting is always on the
menu because the cache can count. The second way in is the **Scenery** — a standing, *reactive*
view that keeps itself current and tells you when it changed. UIs bind to sceneries; chapter 7
lives there.

<svg viewBox="0 0 760 250" xmlns="http://www.w3.org/2000/svg" font-family="sans-serif" font-size="13">
  <defs>
    <marker id="arrow2" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="#888"/>
    </marker>
  </defs>

  <!-- Dio -->
  <rect x="30" y="30" width="180" height="200" rx="12" fill="none" stroke="#8f5a2d" stroke-width="2.5"/>
  <text x="120" y="125" text-anchor="middle" fill="#8f5a2d" font-weight="bold" font-size="15">Dio</text>
  <text x="120" y="148" text-anchor="middle" fill="#888" font-size="12">master + cache</text>

  <!-- facade Vista (pull) -->
  <rect x="540" y="30" width="190" height="70" rx="10" fill="#7c2d8f"/>
  <text x="635" y="59" text-anchor="middle" fill="#fff" font-weight="bold">facade Vista</text>
  <text x="635" y="80" text-anchor="middle" fill="#fff" font-size="12">dio.vista()</text>

  <!-- request: facade → dio -->
  <line x1="540" y1="50" x2="218" y2="50" stroke="#888" stroke-width="2" marker-end="url(#arrow2)"/>
  <text x="375" y="42" text-anchor="middle" fill="#888" font-size="12">proactive: you ask — list, get, count…</text>

  <!-- response: dio → facade -->
  <line x1="210" y1="80" x2="536" y2="80" stroke="#888" stroke-width="2" marker-end="url(#arrow2)"/>
  <text x="375" y="98" text-anchor="middle" fill="#888" font-size="12">…it answers from the cache</text>

  <!-- Scenery (push) -->
  <rect x="540" y="160" width="190" height="70" rx="10" fill="#7c2d8f"/>
  <text x="635" y="189" text-anchor="middle" fill="#fff" font-weight="bold">Scenery</text>
  <text x="635" y="210" text-anchor="middle" fill="#fff" font-size="12">a standing view</text>

  <!-- push: dio → scenery -->
  <line x1="210" y1="195" x2="536" y2="195" stroke="#888" stroke-width="2" marker-end="url(#arrow2)"/>
  <text x="375" y="187" text-anchor="middle" fill="#888" font-size="12">reactive: changes stream in —</text>
  <text x="375" y="215" text-anchor="middle" fill="#888" font-size="12">the generation counter announces each one</text>
</svg>

### The facade Vista

Hand `dio.vista()` to chapter 4's `print_vista` and it just works — it never learns a cache is
underneath. What makes the facade interesting is its capability set. Chapter 4's contract was
honest but rigid: whatever the backend can't do, your application can't have. The facade solves
that — it
carries the master's capabilities, and the Lens can manipulate and extend them. Our read-only S3
listing gains `can_insert` the moment an `on_write` callback gives writes somewhere to go. The
honesty contract still holds; the facade just advertises what the *pipeline* can do, not what the
backend alone can.

|                      | facade Vista                                | Scenery                                          |
| -------------------- | ------------------------------------------- | ------------------------------------------------ |
| **Access style**     | Proactive — you ask, it answers             | Reactive — a standing view that stays current    |
| **Interface**        | Chapter 4's Vista API, unchanged            | Purpose-built: rows by index, a record, a scalar |
| **Freshness**        | As fresh as the cache when you ask          | Recomputes on every change                       |
| **Change awareness** | None — ask again                            | `subscribe()` → generation channel               |
| **Capabilities**     | The master's, plus whatever the Lens adds   | Ordering, search, viewport — regardless of backend |
| **Typical consumer** | Handlers, scripts, CLI commands             | UIs, dashboards, live views                      |

---

## The project: a weather-station inventory

NOAA publishes its daily climate archive — GHCN, one CSV per weather station — as a public S3
bucket in the [AWS Open Data registry](https://registry.opendata.aws/noaa-ghcn/). It's a perfect
Diorama subject: listing it is slow, every listing request is paid again on every run, and the
API can't sort or search. We'll grow one small tool across three chapters:

- **This chapter** — a CLI that lists the station files from a persistent local cache.
- **Chapter 6** — *augmentation*: each file gains columns computed from its contents (how many
  readings, how recent).
- **Chapter 7** — a live terminal UI that scrolls the whole archive and fetches per-file data for
  exactly the rows on screen.

```admonish info title="No AWS account required"
The bucket allows anonymous access, the way `aws s3 ls --no-sign-request` reads it.
`AwsAccount::public(region)` is the vantage-aws equivalent: requests go out unsigned, so there are
no credentials to configure — nothing to install, nothing to pay.
```

Set up the project:

```sh
cargo new learn-4 && cd learn-4
cargo add serde --features derive
cargo add tokio --features full
cargo add vantage-aws vantage-diorama vantage-vista
```

`vantage-aws` is the S3/DynamoDB/IAM driver — it signs (or deliberately doesn't sign) requests
itself, so there's no AWS SDK in the tree. `vantage-diorama` is what this chapter is about. The
code splits into two files: `files.rs` holds the table definition, `main.rs` uses it.

---

## `files.rs` — the listing as a table

One row per file in the bucket. This is chapter 2's pattern — an entity, and a `table()`
constructor that describes the source — pointed at a cloud API instead of a database:

```rust
use serde::{Deserialize, Serialize};
use vantage_aws::prelude::*;

/// One file in the bucket. Field names match S3's wire XML
/// (`<Contents><Key/><Size/></Contents>`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct File {
    #[serde(rename = "Key")]
    pub filename: String,
    #[serde(rename = "Size")]
    pub size: String,
}
```

The serde renames map S3's wire names onto the fields we actually want to write in Rust. `Size`
is a `String` because that's what the XML carries — no silent coercion.

```rust
impl File {
    /// `ListObjectsV2` narrowed to one bucket and prefix. S3 sends at
    /// most `max-keys` keys per response; the `@continuation-token`
    /// cursor tells the framework to keep requesting pages until the
    /// listing is complete.
    pub fn table(aws: AwsAccount, bucket: &str, prefix: &str) -> Table<AwsAccount, File> {
        Table::new(
            "restxml/Contents@continuation-token=NextContinuationToken:s3/GET /{Bucket}?list-type=2",
            aws,
        )
        .with_id_column("Key")
        .with_column_of::<String>("Size")
        .with_condition(eq("Bucket", bucket))
        .with_condition(eq("prefix", prefix))
        .with_condition(eq("max-keys", 100))
    }
}
```

The table *name* is doing a lot of work here — for `vantage-aws`, it's the wire protocol spelled
out: `restxml` is the protocol S3 speaks; `Contents` is the response element holding the rows;
`s3/GET /{Bucket}?list-type=2` is the service and request. Conditions complete the request the
same way chapter 2's conditions completed a WHERE clause: `Bucket` fills the path placeholder,
and anything else (`prefix`, `max-keys`) becomes a query parameter.

The `@continuation-token=NextContinuationToken` part is **auto-pagination**. S3 answers at most
`max-keys` files per response, plus a continuation token when more exist; the cursor declaration
tells the driver to keep re-issuing the request — token folded back in — until the listing is
complete. One `list()` call, as many HTTP requests as it takes.

```admonish info title="The table-name grammar"
The full shape vantage-aws parses is:

~~~text
{protocol}/{array_key}[@cursor|@request=response]:{service}/{METHOD} {path}?{query}
~~~

`protocol` is one of `json1`, `json10`, `query`, `restxml`, `restjson`; `array_key` names the
response element holding the rows; the optional `@` suffix declares the pagination cursor
(one name if request and response fields match, `request=response` when they differ, as here).
Get it wrong and the first query returns a `VantageError` quoting this grammar — an unknown
protocol lists the valid ones. Nothing is guessed and nothing fails silently.
```

## A first listing

`main.rs`, shortest possible version — build the table, list it, print:

```rust
mod files;

use files::File;
use vantage_aws::prelude::*;

const BUCKET: &str = "noaa-ghcn-pds";
const PREFIX: &str = "csv/by_station/GM";

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        e.report();
    }
}

async fn run() -> VantageResult<()> {
    let aws = AwsAccount::public("us-east-1");
    let files = File::table(aws, BUCKET, PREFIX);

    for (filename, file) in files.list().await? {
        println!("{:>10}  {filename}", file.size);
    }
    Ok(())
}
```

The `GM` prefix narrows the archive to Germany's stations — 1122 files, small enough to look at,
big enough to feel:

```text
   4796352  csv/by_station/GM000001153.csv
   7057976  csv/by_station/GM000001474.csv
   2978238  csv/by_station/GM000002277.csv
   ...
1122 files in 2.3s
```

2.3 seconds: twelve HTTPS round-trips (1122 files at 100 per page), paid **on every run**,
forever, because nothing remembers the answer. This is the itch the rest of the chapter
scratches.

---

## The master Vista, and what it can't do

The Dio wants a Vista for its master, so we erase the typed table the same way chapter 4 did:

```rust
let master = aws
    .vista_factory()
    .from_table(File::table(aws.clone(), BUCKET, PREFIX))?;
```

Ask this Vista what it can do, and you'll see why this backend needs a Diorama:

```rust
let caps = master.capabilities();
// can_count: true    can_fetch_next: true — and that's it.
// can_search: false   can_order: false   can_insert: false
```

`can_fetch_next` is worth pausing on — it's the one capability this chapter leans on.
`fetch_next(token)` is Vista's *cursor-style* pagination from chapter 4: pass `None` for the
first page, pass back the returned token for the next one. For S3 the driver defines the token
as **the last key of the previous page**, because S3 lists keys in order and accepts any key as
a starting point (`start-after`). That has a consequence the generic contract doesn't promise:
this cursor survives process restarts. Any key you already hold — say, the last key in a cache —
resumes the listing right after it.

## The Lens: pump pages, resume where you left off

Now the Lens. One callback — on start, synchronize the cache with the bucket:

```rust
let lens = Arc::new(
    Lens::new()
        .cache_at("cache.redb")
        .on_start(|dio| {
            let dio = dio.clone();
            async move { sync(&dio).await }
        })
        .build()
        .context("Failed to build lens")?,
);
let dio = lens.make_dio(master).await?;
```

- **`.cache_at("cache.redb")`** opens a [redb](https://docs.rs/redb) file on disk. This is why
  the cache survives process restarts. An in-memory alternative, `.cache_in_memory()`, is there
  for when persistence isn't wanted.
- **`.on_start(...)`** fires once, inside `make_dio`. Callbacks receive `&Dio` and clone it to
  hold across `.await` — a cheap `Arc` bump; every Lens callback follows this shape.
- **`make_dio`** does the rest in one call: opens a cache table named after the master, spawns
  the write worker, runs `on_start`, and hands back the [`Dio`](vantage_diorama::Dio).

`sync` is where the cursor pays off. Instead of one big `list_values()`, it pulls the listing
**one page at a time** and seeds the cursor from the cache:

```rust
/// Pump the master listing into the cache, one page per request. S3's
/// paging cursor is simply "the last key seen" — so the last key already
/// in the cache resumes the listing, and pages loaded by an earlier run
/// (even one that was interrupted) are never fetched again.
async fn sync(dio: &Dio) -> VantageResult<()> {
    let mut token: Option<CborValue> = dio
        .cache()
        .list_values()
        .await?
        .keys()
        .last()
        .cloned()
        .map(Into::into);
    loop {
        let start = Instant::now();
        let (page, next) = dio.master().fetch_next(token).await?;
        let count = page.len();
        dio.cache().insert_values(page.into_iter().collect()).await?;
        println!("fetched {count} files in {:?}", start.elapsed());
        if next.is_none() {
            return Ok(());
        }
        token = next;
    }
}
```

Read the first statement again: the initial token is *the last filename already cached*. On a
cold cache that's `None` — start from the top. On a warm cache it's the end of what we have —
S3 continues from there. Kill the process halfway through a sync and run it again: the pages you
already paid for are never fetched twice. The cache isn't just an answer store; it's the resume
point.

## Reads come from the cache

The listing loop stops asking S3 and starts asking the Dio — through the **facade Vista** from
the first half of the chapter. Notice who talks to what: `sync`, being Lens plumbing, addresses
the Dio's two sides directly (`dio.master()` to fetch, `dio.cache()` to store and resume);
a *consumer* asks `dio.vista()` and never learns what's underneath. The records come back as
`Record<CborValue>` — the erased form from chapter 4 — so field access goes through the
`CborValueExt` helpers the prelude brings in:

```rust
let start = Instant::now();
let listing = dio.vista().list_values().await?;
for (filename, file) in &listing {
    let size = file.get("Size").and_then(|v| v.as_str()).unwrap_or("");
    println!("{size:>10}  {filename}");
}
println!("{} files from cache in {:?}", listing.len(), start.elapsed());
```

First run — cold cache, the sync pump narrating each page:

```text
fetched 100 files in 215ms
fetched 100 files in 217ms
...
fetched 22 files in 114ms
   4796352  csv/by_station/GM000001153.csv
   7057976  csv/by_station/GM000001474.csv
   ...
1122 files from cache in 18ms
```

Second run — same command, new process:

```text
fetched 0 files in 370ms
1122 files from cache in 17ms
```

One round-trip — the resume request from the last cached key, confirming nothing new exists —
and then the full listing in 17 milliseconds. New files that *do* appear under the prefix arrive
on exactly that request, without re-fetching the thousand we already hold.

## Invalidating

A resuming cache has one blind spot: it only ever looks *past* what it holds, so files deleted
from the bucket linger locally. The honest fix is to start over — wipe and re-pump:

```rust
if std::env::args().any(|a| a == "--invalidate") {
    dio.cache().clear().await?;
    sync(&dio).await?;
}
```

```text
$ cargo run -- --invalidate
fetched 0 files in 370ms
fetched 100 files in 215ms
...
1122 files from cache in 18ms
```

The CLI is complete: two seconds once, milliseconds forever after, resumable mid-sync, and an
escape hatch back to the truth.

---

## The event bus

One more thing `make_dio` set up, invisibly: every Dio carries a **broadcast event bus**.
Anything that changes data announces it there as a [`DioEvent`](vantage_diorama::DioEvent) —
`DatasetChanged` when the set of records was rewritten wholesale, `RecordChanged` /
`RecordInserted` / `RecordRemoved` for row-level changes, `Refreshing` when a reconcile starts,
`WriteFailed` when a queued write fails. Nothing in this chapter listens yet; chapter 7's
sceneries subscribe to this bus to know when to recompute, and chapter 6's hydration uses it to
report progress.

One rule is worth forming as a habit now: **direct cache writes are silent**. Our `sync` calls
`dio.cache().insert_values(...)`, which stores rows and announces nothing — harmless while
nothing listens, invisible data the moment something does. When observers exist, either use the
Dio's row-level helpers that write *and* announce in one motion — `patched(id, record)`,
`removed(id)` — or follow a bulk cache write with `notify_dataset_changed()`, the "re-read
everything" announcement. Chapter 7's live view does exactly that.

```admonish info title="Poll or push — the Lens doesn't care"
Our S3 listing has no way to notify us, so freshness is *pulled*: `sync` on start,
`--invalidate` by hand, or `refresh_every(duration)` on a timer (chapter 7's live view uses it).
Backends that can push — a SurrealDB live query, a Kafka topic, a webhook — feed
[`ChangeEvent`](vantage_diorama::ChangeEvent)s into `dio.handle_event(...)` and reconcile through
an `on_event` callback instead, using the same row-level helpers. Same cache, same events — only
the trigger differs.
```

```admonish info title="Writes, on a read-only master"
S3's listing Vista advertises `can_insert: false`, and the facade won't pretend otherwise — by
default. Register an **`on_write`** callback on the Lens and the write queue becomes yours:
each queued [`WriteOp`](vantage_diorama::WriteOp) is handed to your closure, which can append to
a journal, call a different API, or write a queue — this is exactly the introduction's
"a read-only CSV file accepts writes by routing them into a queue". Without `on_write`, queued
ops are applied to the master directly, and a master that can't take them surfaces
`DioEvent::WriteFailed` on the bus — never a silent drop.
```

## Callback summary

| Callback         | When it fires              | In this chapter                       |
| ---------------- | -------------------------- | ------------------------------------- |
| `on_start`       | Once at `make_dio`         | `sync` — pump pages, resume from cache |
| `on_refresh`     | `refresh()` + timer        | (not used — chapter 7 reconciles on a timer) |
| `on_write`       | Every `WriteOp`            | (not used — S3 listing is read-only)  |
| `on_event`       | Upstream `ChangeEvent`     | (not used — S3 can't push)            |
| `on_list_page`   | Scenery list pass          | Chapter 7                             |
| `on_load_detail` | Scenery detail pass        | Chapter 7                             |
| `total_provider` / `on_load_chunk` | Scenery loading | Chapter 7                          |

---

## What we covered

| Concept                                             | What it does                                                    |
| --------------------------------------------------- | ---------------------------------------------------------------- |
| `AwsAccount::public(region)`                        | Unsigned requests — public buckets, no credentials               |
| Cursor in the table name (`@req=resp`)              | Auto-pagination: one `list()`, as many requests as needed        |
| [`Lens`](vantage_diorama::Lens)                     | Shared infrastructure: cache, callbacks, refresh policy          |
| [`Dio`](vantage_diorama::Dio)                       | Binding of master Vista + Lens; owns cache, queue, event bus     |
| `cache_at` / [`CacheBackend`](vantage_diorama::CacheBackend) | Persistent (redb) or in-memory storage for cached rows  |
| `fetch_next(token)`                                 | One page per call; S3's token is the last key — durable          |
| `dio.cache()` / `dio.master()`                      | The two sides of the Dio, directly addressable from callbacks    |
| `dio.vista()`                                       | Facade Vista: reads from cache, writes through the queue         |
| [`DioEvent`](vantage_diorama::DioEvent)             | Bus event: invalidated, record-level changes, write failures     |

```admonish tip title="What's next"
The inventory knows every file's name and size — and nothing about what's inside. The next
chapter teaches the Dio to *augment* its rows: each station file gains a reading count and a
latest-reading date, computed from the file itself, fetched once, cached forever.
```
