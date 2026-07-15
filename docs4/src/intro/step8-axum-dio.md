# Serving Scenery — Axum & Watch Streams

Chapter 7's client of the Dio was a terminal driving two sceneries — one showing the rows, the
other aggregating a running sum. Sceneries scale well beyond that: the framework can drive many
active viewers at once, and nothing requires them to live in the same process. This chapter
sends them across the network — every connected browser tab becomes its own viewer, looking at
its own page of the archive, with details streaming in as they land.

The traditional plumbing for live updates over HTTP is a
[WebSocket](https://en.wikipedia.org/wiki/WebSocket),
[server-sent events](https://en.wikipedia.org/wiki/Server-sent_events), or the style the
Kubernetes API made standard — the **watch**: a plain HTTP response that simply never ends,
delivering one JSON line per change over
[chunked transfer encoding](https://en.wikipedia.org/wiki/Chunked_transfer_encoding). We take
the Kubernetes shape: every endpoint answers a plain `GET` with a snapshot, and the same URL
with `?watch=true` keeps the connection open and streams changes for as long as the client
stays. A watch is not a polling loop — it is a Scenery on the far end of an HTTP connection.

This chapter re-uses chapter 3's Axum server, wired to the Dio through a new adapter crate —
and finishes with a small React app browsing the bucket the way the AWS S3 Explorer does. Worth
pausing on what the frontend gets for free: the page renders instantly from the
cache, cells fill themselves in as augmentation lands, and every open tab stays current — the
responsiveness chapter 7 built for the terminal, now delivered over a wire.

## One flight per row

Before any of that can be safe, the framework needs an answer to a question chapter 7 never had
to ask: what happens when **several** views drive augmentation at once?

Until now, each scenery ran its own detail fetches, inline, for its own viewport. One terminal,
one viewport — fine. But two browser tabs watching *overlapping* pages would each download the
same CSVs; two tabs on *disjoint* pages would race each other with no ordering at all; and a
tab opened mid-download had no way to say "me next". The fetches were nobody's job to
coordinate.

They are now the Dio's. Every consumer that wants rows hydrated — a scenery's viewport, a
facade read blocking on its window — registers a queue with the Dio's **augment scheduler** and
enqueues row ids into it. A worker pool drains the queues:

- **Round-robin across consumers.** The worker takes one id from each queue in turn, so two
  views with disjoint pages interleave — neither starves behind the other's backlog.
- **One flight per row.** An id already being fetched is never fetched again; every queue
  waiting on it is notified by the same completion. Overlapping views cost one download, not
  one per view.
- **Closing a view withdraws its work.** The queue registration is owned by the scenery; when
  the last handle drops, its queued ids vanish. A fetch already in the air completes and lands
  in the cache — paid-for work is kept.
- **Workers are configurable.** The default single worker keeps fetch order deterministic;
  `Lens::new().augment_workers(4)` hydrates four rows at a time when the detail source can
  take it.

Nothing in the example code changes for this — the scheduler sits under the same
`set_viewport` and `fetch_window` calls the previous chapters used. What changes is what you
can now safely do: open as many sceneries as you have HTTP connections.

One builder flag is new. Sceneries de-duplicate: two identical opens share one instance, which
is right for two widgets showing the same grid — but wrong for two *clients* watching different
pages of the same query, where each connection must keep its own viewport. The adapter opens
its sceneries with `.exclusive()`: never shared, still counted in the **demand union** — the
Dio hydrates the union of the columns every open scenery demands — and still released on drop.

## The adapter: `DioRouter`

Everything HTTP lives in a new adapter crate, `vantage-api-adapters` — the server-side sibling
of the `dataset-ui-adapters` crate that provided chapter 7's ratatui binding. Where a UI
adapter binds a scenery to a widget, an API adapter binds it to a route:

```rust
use vantage_api_adapters::axum_dio::DioRouter;

let api = DioRouter::new(dio.clone())
    .with_column("filename", "Key")
    .with_column("size", "Size")
    .with_column("rows", "rows")
    .with_column("latest", "latest")
    .with_page_size(50)
    .into_router();
```

Each `.with_column(name, field)` maps a record field to a JSON key — and doubles as the watch
sceneries' *demand*: naming `rows` and `latest` here is exactly what makes a watch connection
drive their hydration (chapter 7's demand gate, now per connection). `into_router()` yields a
plain `axum::Router` with two routes, each in two modes:

| Request                        | Answered by                               | Cost                          |
| ------------------------------ | ----------------------------------------- | ----------------------------- |
| `GET /?offset=&limit=`         | A window over the cache                   | Instant; never fetches        |
| `GET /?watch=true&…`           | An `.exclusive()` `TableScenery`          | Streams while connected       |
| `GET /{id}`                    | A bounded facade read (`get_value`)       | Hydrates that one row; cached |
| `GET /{id}?watch=true`         | A `RecordScenery`                         | Streams that record's changes |

The split embodies the demand philosophy: a plain `GET` is not a standing view, so it serves
the Dio's *current knowledge* instantly — augmented columns appear once some view has paid for
them. A **watch is the standing view**: it declares its page as the viewport, hydration
follows, and every change streams back as a Kubernetes-style NDJSON line:

```text
{"type":"ADDED","object":{"index":3,"filename":"…","size":"4880965","rows":null,"latest":null}}
{"type":"MODIFIED","object":{"index":3,"filename":"…","size":"4880965","rows":143676,"latest":"20260531"}}
```

The stream diffs against what it already sent — a generation bump that changed nothing on this
page costs nothing on the wire. And the scenery is *owned by the response stream*: when the
client disconnects, the stream drops, the scenery's guard aborts its tasks, its queued fetches
are withdrawn, and its demand drains. A closed tab stops pulling — the same lifecycle rule as
chapter 7's closing grid, now enforced by HTTP.

One honesty note against the Kubernetes original: there is no resume token (no
`resourceVersion`). A client that reconnects gets a fresh snapshot of `ADDED` lines and a new
watch — not a replay of what it missed while away.

## The server

`learn-7` is learn-6 with the terminal swapped for the router — and the prefix narrowed back to
chapter 5's `GM` (1,122 files): a server should boot in seconds, and everything below works the
same way on the full archive. The data plumbing is otherwise identical —
`files.rs`, the augmenter, the Lens — with two differences. First, the cache is opened by hand,
because we want a second table in the same redb file:

```rust
let cache = Arc::new(RedbCache::open("cache.redb").context("Failed to open cache")?);
let contents = ContentsCache::new(cache.open_table("contents").await?);
```

A Lens normally opens `cache.redb` itself (`cache_at`); `cache_source(cache)` hands it ours
instead. The Dio claims one named table for the listing, and `open_table("contents")` claims
another for the cache we're about to meet.

Second, `on_start` stays **blocking** (the default — learn-6 turned it off to open its UI on an
empty table). A server should answer its first request from a warm cache, so `make_dio` runs
the chapter-5 sync to completion before `axum::serve` ever binds the port. On a restart the
sync resumes from redb and confirms with a single request:

```text
fetched 1000 files in 831.454208ms
fetched 122 files in 194.010875ms
serving on http://localhost:3007
```

The rest of `main.rs` is mounting:

```rust
let app = axum::Router::new()
    .nest("/api/files", api)
    .fallback_service(ServeDir::new("frontend/dist"));

axum::serve(TcpListener::bind("0.0.0.0:3007").await?, app).await
```

Concurrency needs no further code. `Dio` is a cheap clone over shared state, redb reads run
concurrently, and no lock is held across a network await anywhere in the read path — every
simultaneous request simply proceeds.

Neither does authentication — because there is none. The endpoints are anonymous, as in
chapter 3: wrap the router in your tower auth middleware before exposing it; Vantage
deliberately stays out of authn.

## A cache that earns its keep

The detail endpoint returns everything about one file, augment columns included — which means
the first `GET /{id}` downloads the CSV. Chapter 6 deliberately kept `contents` out of the
cache: at 122,000 stations, storing every casually-viewed file would grow `cache.redb` by
gigabytes. But a server sees *repeat* traffic — the same popular stations, again and again —
and re-downloading those is just as wasteful.

`ContentsCache` splits the difference with **lazy admission**: a file must be requested twice
before its contents earn a slot. It is application code, not framework — the framework
contributes `open_table` (a named key-value table in the Dio's own redb file); the admission
policy is entirely ours to write.

```rust
pub struct ContentsCache {
    table: Arc<dyn CacheTable>,
    /// Keys downloaded at least once — the admission ledger.
    seen: Mutex<HashSet<String>>,
}
```

Its one operation is cache-first with the admission rule on the miss path:

```rust
pub async fn get_or_fetch<F, Fut>(&self, key: &str, fetch: F) -> VantageResult<String>
```

A hit is served from redb. A miss runs `fetch` either way — but only a key already in `seen` (a
repeat request) gets its body written to the `contents` table. One-off requests leave nothing
behind but their key. (The ledger itself is never trimmed — bare keys, fine at 122,000 files; a
bigger keyspace would want an eviction rule, and the policy being application code makes that
your call.) The augmenter's `contents` lazy expression routes its download through
it, and everything downstream — `rows`, `latest`, the detail endpoint — is unchanged:

```rust
.with_lazy_expression("contents", move |row| {
    // …
    async move {
        let body = contents
            .get_or_fetch(&key, || async move {
                s3::get_object(&aws, &bucket, &fetch_key).await
            })
            .await?;
        Ok(body.into())
    }
})
```

## Watching it work

A plain `GET` is the cache, instantly — `rows` and `latest` are `null` because nothing has
demanded them yet:

```text
$ curl 'localhost:3007/api/files?offset=0&limit=3'
{"total":1122,"offset":0,"limit":3,"items":[
  {"index":0,"filename":"csv/by_station/GM000001153.csv","size":"4796352","rows":null,"latest":null},
  …
```

A detail `GET` blocks on the bounded facade read — 1.8 s to download and digest 4.8 MB — and
the same request again is a 21 ms cache hit:

```text
$ curl 'localhost:3007/api/files/csv%2Fby_station%2FGM000001153.csv'
{"Key":"csv/by_station/GM000001153.csv", …"Size":"4796352", …"rows":140629,"latest":"19911231"}
```

Now the standing view. `curl -N` holds the connection; the page arrives as `ADDED` rows, then
each CSV lands as a `MODIFIED` line the moment its download completes:

```text
$ curl -N 'localhost:3007/api/files?offset=3&limit=3&watch=true'
{"type":"ADDED","object":{"index":3,"filename":"csv/by_station/GM000002288.csv","size":"4880965","rows":null,"latest":null}}
{"type":"ADDED","object":{"index":4,"filename":"csv/by_station/GM000002698.csv","size":"6351302","rows":null,"latest":null}}
{"type":"ADDED","object":{"index":5,"filename":"csv/by_station/GM000002716.csv","size":"5042384","rows":null,"latest":null}}
{"type":"MODIFIED","object":{"index":3,…"rows":143676,"latest":"20260531"}}
{"type":"MODIFIED","object":{"index":4,…"rows":186088,"latest":"20081031"}}
{"type":"MODIFIED","object":{"index":5,…"rows":147011,"latest":"20250824"}}
```

And the scheduler is visible from outside. Two watches on **disjoint** pages, side by side:
their `MODIFIED` lines alternate — B, A, B, A — the single worker taking one row from each
view's queue in turn:

```text
19:35:35 [B] {"type":"MODIFIED","object":{"index":42,…"rows":201382,…}}
19:35:36 [A] {"type":"MODIFIED","object":{"index":40,…"rows":77595,…}}
19:35:38 [B] {"type":"MODIFIED","object":{"index":43,…"rows":132579,…}}
19:35:40 [A] {"type":"MODIFIED","object":{"index":41,…"rows":128335,…}}
```

Two watches on the **same** page: each row's `MODIFIED` reaches both connections in the same
second — one download, fanned out by the event bus to every scenery holding the row:

```text
19:36:18 [B] {"type":"MODIFIED","object":{"index":50,…"rows":135802,…}}
19:36:18 [A] {"type":"MODIFIED","object":{"index":50,…"rows":135802,…}}
```

Run the first `GET` again afterwards: the page the watches paid for now answers with its
numbers filled in. Current knowledge grew.

## A tiny React client

The frontend is one component. It fetches its page with `watch=true` and reads the NDJSON
stream directly off `fetch` — no client library, twenty lines:

```jsx
const res = await fetch(`/api/files?offset=${offset}&limit=${LIMIT}&watch=true`, {
  signal: ctl.signal,
})
const reader = res.body.getReader()
// …accumulate chunks, split on '\n'…
const event = JSON.parse(line)
setRows(rs => ({ ...rs, [event.object.index]: event.object }))
```

`ADDED` and `MODIFIED` merge the same way — the object lands at its index — so the table
renders the listing immediately and the `Rows` / `Latest` cells flip from `…` to numbers as the
watch delivers them, exactly like chapter 7's terminal cells did. Paging aborts the fetch via
the `AbortController`, which closes the connection, which drops the scenery server-side.
Clicking a row hits the detail endpoint into a side panel. `npm run build` in `learn-7/frontend`
emits `dist/`, the `ServeDir` fallback serves it, and `http://localhost:3007` is a bucket
explorer: title, file count, Prev/Next, and a table filling itself in.

---

## What we covered

| Concept                                  | What it does                                                            |
| ---------------------------------------- | ----------------------------------------------------------------------- |
| Augment scheduler                        | Dio-owned detail fetches: round-robin across views, one flight per row  |
| `augment_workers(n)`                     | Worker pool size — 1 (deterministic order) by default                   |
| `.exclusive()`                           | A scenery that never shares — one standing view per HTTP connection     |
| `DioRouter` (vantage-api-adapters)       | `.with_column()` + `.with_page_size()` → an axum router; columns double as demand |
| `GET` vs `?watch=true`                   | Snapshot of current knowledge vs a Scenery streaming NDJSON events      |
| `ADDED` / `MODIFIED` lines               | Kubernetes-style watch events, diffed per row per connection            |
| Connection drop = scenery drop           | Queued fetches withdrawn, demand drained — a closed tab stops pulling   |
| Blocking `on_start`                      | Pre-fetch: the port opens only once the listing is cached               |
| `cache_source` + `open_table`            | One redb file, many named tables — the Dio's and our own                |
| `ContentsCache`                          | Lazy admission: downloaded once — remembered; twice — cached            |

```admonish success title="The whole climb"
Eight chapters ago this book started with one SQL query; it ends with a fair-scheduled,
cache-backed watch API feeding a React frontend — and every layer still speaks through the one
below it.

From here, the reference half of the book takes over — [Augmentation](../augmentation.md) for
batched fetches and demand gating, [Config-Driven Vistas](../config-driven-vistas.md) for
defining all of this from YAML, [Model-Driven Architecture](../mda.md) for structuring a real
application, and [Adding a New Persistence](../new-persistence.md) when you're ready to extend
the framework itself.
```
