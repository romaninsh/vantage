# Implementing a Data Source

This document is for someone writing a new Vista driver — a `TableShell`
implementation that wraps a particular backend (a REST API, a custom binary
file format, a niche database). It explains what your driver needs to do to
play well with Diorama, and what Diorama compensates for so you don't have
to.

If you haven't written a Vista driver yet, start with the persistence
walkthrough at `docs4/src/new-persistence/step8-vista-integration.md` and
read the [`TableShell` trait](../vantage-vista/src/source.rs) — that's the
canonical contract. This document picks up where those leave off.

> **Implementation status.** The contracts described below all sit on
> Vista's `TableShell`; the actual method names (`list_vista_values`,
> `get_vista_value`, `fetch_page`, `fetch_next`, `add_eq_condition`,
> `add_order`, `add_search`, `get_ref`, etc.) live there. The
> `Page<Id, Rec>` shape sketched in this doc maps onto vista's `fetch_page`
> / `fetch_next` (token-based) pagination — see vista's
> [`source.rs`](../vantage-vista/src/source.rs) for the live signatures.

## The shortest possible Diorama-compatible driver

A driver that satisfies Vista's `TableShell` trait and reports honest
capabilities is automatically Diorama-compatible. There is no separate
"Diorama trait" to implement. What changes is the way you think about your
driver's role: Diorama will wrap it, compose it, cache reads against it, and
ask hard questions about what it can and can't do. Your job is to answer
those questions accurately.

The four things that matter:

1. **Advertise capabilities honestly.**
2. **Implement `fetch_one` reliably.**
3. **Support cursor-based pagination if your backend has any pagination at
   all.**
4. **Optionally implement change subscription.**

Everything else — sort, search, filter, persistence, retries, optimistic
writes — Diorama can compensate for. You don't have to.

## What Diorama needs from your Vista

### Capability advertisement

`VistaCapabilities` is the contract. Every flag on it tells Diorama what your
driver can do natively versus what Diorama has to fill in:

```rust
pub struct VistaCapabilities {
    pub can_count: bool,
    pub can_insert: bool,
    pub can_update: bool,
    pub can_delete: bool,
    pub can_subscribe: bool,
    pub can_invalidate: bool,
    pub can_order: bool,
    pub can_search: bool,
    pub can_set_page_size: bool,
    pub can_fetch_page: bool,
    pub can_fetch_next: bool,
}
```

Report what's true. If your driver can sort on indexed columns but not on
arbitrary ones, set `can_order = true` and reject unsupported sorts at
execution time with a clear error. If your driver returns up to N rows per
request but has no concept of "next page," set `can_fetch_page = true` and
`can_fetch_next = false`. Diorama uses these flags to decide what to push
down and what to handle itself.

### Honest errors

When something doesn't work, return a structured error. Don't panic, don't
return empty results that pretend the operation succeeded, don't return
partial data without a continuation cursor.

Diorama's `on_query` and `on_refresh` callbacks treat errors as data — they
log, mark the cache stale, emit events. A driver that fakes success robs the
user of the chance to handle the failure.

### Reliable `fetch_one`

Many of Diorama's optimizations depend on fetching a single record by id
cheaply. Even if your backend doesn't have a native "get by id" — e.g., it
only returns paginated lists — implement `fetch_one` by scanning. Document the
cost so users know it's not free; but ship it.

```rust
async fn fetch_one(&self, id: &RecordId) -> Result<Option<Record<CborValue>>> {
    // Native:
    self.api.get(format!("/products/{id}")).await.into_record()

    // Fallback if backend has no GET-by-id:
    // self.list_all().await?.into_iter().find(|r| r.id() == *id)
}
```

If your driver advertises `can_subscribe = true`, Diorama may call
`fetch_one(id)` in response to a change event to repopulate a single cache
slot. A correct `fetch_one` is what makes incremental cache updates possible.

### Cursor-based pagination, if you have any

Diorama treats pagination as a per-driver opt-in. If your backend supports it
in any form — page numbers, offset/limit, opaque continuation tokens — wrap it
in the cursor protocol:

```rust
pub struct Page<Id, Rec> {
    pub rows: Vec<(Id, Rec)>,
    pub next_cursor: Option<Vec<u8>>,
    pub total_estimated: Option<u64>,
}

async fn fetch_page(
    &self,
    query: &Query,
    cursor: Option<Vec<u8>>,
    limit: usize,
) -> Result<Page<RecordId, Record<CborValue>>>;
```

The cursor is your driver's private business — Diorama doesn't inspect it,
just stores it and hands it back. Use whatever encoding makes sense (page
number, last-seen-id, opaque token) and serialize to bytes.

`total_estimated` is optional. If your backend tells you "approximately
12,000 results match," report it for the UI scrollbar. If you don't know,
return `None` — Diorama handles unknown counts (see `README_ui.md` for the
infinite-scroll model).

### What if my backend has no pagination at all?

Set `can_fetch_page = false` and implement a single batch fetch. Diorama will
load everything in one shot and cache it. This is fine for sources with
bounded data sizes (config tables, lookup data, small datasets). For sources
that could return millions of rows without pagination, document the risk —
the user should know not to wrap an unbounded source.

### Optional: change subscription

If your backend can push changes — SurrealDB LIVE queries, Postgres
LISTEN/NOTIFY, Kafka topics, websocket feeds — implement the subscription
hook:

```rust
fn subscribe(&self) -> Pin<Box<dyn Stream<Item = ChangeEvent> + Send>>;
```

The events you emit drive Diorama's `on_event` callbacks. Granularity matters:
emit per-record events (`ChangeEvent::Updated { id, new }`) when your backend
provides them; fall back to `ChangeEvent::Invalidated` for wholesale "something
changed" notifications. The latter forces full refreshes; the former lets
Diorama update individual cache slots.

If your backend doesn't push changes, leave `can_subscribe = false`. Diorama
will use polling refresh instead. Users wire the refresh interval in the
Lens.

## What Diorama handles for you

You don't need to implement any of these. If a user wraps your Vista in a
Diorama, the Lens fills in:

- **Caching.** Disk-backed, in-memory, or anywhere else the user points it.
  Reads come from the cache; your driver only sees a request when the cache
  is cold.
- **Retries.** Failed operations are retried by user policy. Your driver
  should return real errors and let the user decide.
- **Rate limiting.** Users can stack a `RateLimitedDio` in front of your
  driver if they need it.
- **Local sort, search, filter.** When the user's Lens wraps your Vista with
  a sort capability and your driver doesn't support it, Diorama loads the
  data and sorts in memory.
- **Write coalescing.** If a user makes ten edits to the same record in
  quick succession, Diorama can coalesce them before your driver sees a
  single write. Driver-side, you see whatever the user's `on_write` callback
  decides to send.
- **Optimistic UI updates.** Sceneries reflect a write the instant it's
  enqueued; your driver sees it on the worker thread after.
- **Live invalidation propagation.** Even if your driver can't subscribe,
  users can plug in external event sources (Kafka, MQTT, custom websockets)
  via the Lens's `on_event`. Your driver just needs to refresh accurately
  when asked.

The dividing line: **your driver is the source of truth and the only thing
that talks to the backend**. Everything time-shaped — caches, queues, timers,
retries — lives in the Lens.

## Capability cheatsheet

Map your driver's natural shape to the right flags:

| Your backend                              | Capability flags to set                       |
|-------------------------------------------|------------------------------------------------|
| Read-only file (CSV, parquet)             | `can_fetch_page` if your reader can chunk     |
| REST API with limit/offset                | `can_fetch_page = true`, `can_fetch_next = true` |
| REST API with cursor tokens               | same; encode the token as bytes               |
| GraphQL with cursor pagination            | same                                          |
| KV store (DynamoDB, Redis)                | `can_fetch_page = false` usually; `can_count = false` |
| SQL with indexed columns                  | `can_order = true` on indexed cols only       |
| Search engine (Elasticsearch, Meilisearch)| `can_search = true`, `can_order = true`       |
| Event log (Kafka, Pulsar)                 | `can_subscribe = true`; `can_fetch_page = true` only if the topic supports replay |
| In-memory mock                            | everything `true` — the easy case             |

The MongoDB driver advertises `can_order` and `can_search` because the
backend handles both natively. The CSV driver advertises neither — Diorama
sorts and filters in memory if a user wraps a CSV in a sort-capable Lens.
This is the model: be honest about what you do; Diorama fills the rest.

## Implementing change subscription against a backend that doesn't have one

Sometimes you'll want to wrap a backend that has no native change stream —
say, a REST API that's polled. You can still build a `ChangeEvent` stream
synthetically: poll periodically, diff against the last seen, emit events for
what changed.

But this isn't the driver's job. Leave `can_subscribe = false` and let the
user configure polling refresh on the Lens. Synthetic event streams are a
user-level policy decision (how often to poll, how to dedupe, how to handle
deletes that aren't in the diff). Don't bake assumptions into the driver
that the user might want to override.

## Testing your driver against Diorama

A quick integration check: wrap your Vista in a minimal Lens and exercise
the surface.

```rust
use std::time::Duration;
use vantage_diorama::Lens;

let lens = Lens::new()
    .cache_at("./test-cache.redb")
    .on_start(|dio| async move {
        let data = dio.master().list_values().await?;
        dio.cache().insert_values(data).await?;
        Ok(())
    })
    .on_write(|dio, op| async move {
        dio.master().apply(&op).await?;
        dio.refresh().await?;
        Ok(())
    })
    .refresh_every(Duration::from_secs(10))
    .build()
    .await?;

let dio = lens.make_dio(my_driver.vista()?);

// Read should hit the cache after first call.
let rows = dio.vista().list_values().await?;
assert!(!rows.is_empty());

// Write should round-trip through your driver.
let new = sample_record();
dio.vista().insert(new.clone()).await?;
dio.refresh().await?;
let rows = dio.vista().list_values().await?;
assert!(rows.iter().any(|r| r.id() == new.id()));

// Capability flags should be reported correctly.
assert_eq!(dio.vista().capabilities().can_subscribe, true /* if driver advertises it */);
```

If reads work, writes round-trip, and the Lens's `on_start` /`on_refresh` /
`on_write` all fire as expected, your driver is Diorama-compatible. The full
test matrix (composition with other drivers, capability propagation, error
surfaces) lives in `vantage-diorama/tests/`.

## A small worked example

A REST API for a product catalog. Pagination via `?page=N`. No native search.
No subscription.

```rust
use vantage_vista::{Vista, VistaCapabilities, TableShell};

pub struct RestVista {
    base_url: String,
    client: reqwest::Client,
}

#[async_trait]
impl TableShell for RestVista {
    fn capabilities(&self) -> VistaCapabilities {
        VistaCapabilities {
            can_count: true,            // /products/count exists
            can_insert: true,
            can_update: true,
            can_delete: true,
            can_fetch_page: true,
            can_fetch_next: true,
            // The rest default to false.
            ..Default::default()
        }
    }

    async fn fetch_page(
        &self,
        _query: &Query,
        cursor: Option<Vec<u8>>,
        limit: usize,
    ) -> Result<Page<RecordId, Record<CborValue>>> {
        let page: u32 = cursor.as_ref()
            .map(|c| u32::from_le_bytes(c[..4].try_into().unwrap_or([0; 4])))
            .unwrap_or(0);
        let resp: ApiResponse = self.client
            .get(format!("{}/products?page={page}&limit={limit}", self.base_url))
            .send().await?
            .json().await?;
        Ok(Page {
            rows: resp.items.into_iter().map(|p| (p.id.clone(), p.into_record())).collect(),
            next_cursor: resp.next_page.map(|n| n.to_le_bytes().to_vec()),
            total_estimated: resp.total,
        })
    }

    async fn fetch_one(&self, id: &RecordId) -> Result<Option<Record<CborValue>>> {
        let url = format!("{}/products/{id}", self.base_url);
        match self.client.get(url).send().await?.error_for_status() {
            Ok(r) => Ok(Some(r.json::<Product>().await?.into_record())),
            Err(e) if e.status() == Some(reqwest::StatusCode::NOT_FOUND) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // insert / update / delete similarly.
}
```

A user wraps this in a Lens that adds caching and search-in-memory:

```rust
let lens = Lens::new()
    .cache_at("./products-cache.redb")
    .on_start(|dio| async move {
        let mut cursor = None;
        loop {
            let page = dio.master().fetch_page(&Query::all(), cursor, 100).await?;
            dio.cache().insert_values(page.rows.into_iter().map(|(_, r)| r).collect()).await?;
            cursor = page.next_cursor;
            if cursor.is_none() { break; }
        }
        Ok(())
    })
    .refresh_every(Duration::from_secs(900))
    .build()
    .await?;

let products = lens.make_dio(rest_vista.into_vista()?);

// Search works even though the REST API doesn't support it — Diorama uses the cache.
let cakes: Vec<_> = products.vista()
    .add_search("cake")
    .list_values()
    .await?;
```

The driver did the minimum. Diorama provides the rest.
