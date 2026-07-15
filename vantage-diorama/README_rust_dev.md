# Using Diorama from Business Logic

This document is for someone writing Rust code that isn't a UI — an API
handler, a CLI tool, a batch job, an internal library. You have a Vista
(or a few of them) and you want to do useful work with the data behind
them. Reactivity isn't on the table; you want straightforward fetches and
mutations that happen to be fast and resilient.

Short version: keep using Vista the way you already do. Diorama wraps your
Vista in a Lens when you need caching or offline tolerance, and you get back
a Vista that behaves the same way, just better.

> **Implementation status.** The "minimum useful Diorama", the "Patterns by
> shape of work" → "Internal library" / "Batch job" / "CLI tool" sections,
> and the worked axum example use APIs that are mostly real but mention
> a few v1 caveats: `on_query` is registered but not invoked until vista
> stage 5b lands, and there is no shipped in-memory `CacheBackend` (everything
> persists to redb today). Build a redb cache in a `tempdir` if you need an
> ephemeral one.

## The minimum useful Diorama

You have a slow API. Your handler hits it on every request. You want to
cache reads for sixty seconds and have writes invalidate the cache.

```rust
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use vantage_diorama::{Lens, ops::WriteOp};

// Pre-stage-5b: persistent in-memory caches aren't shipped yet — point
// `cache_at` at a tempdir if you want a per-process cache.
let cache_dir = TempDir::new()?;
let lens = Arc::new(
    Lens::new()
        .cache_at(cache_dir.path().join("api.redb"))
        .on_start_blocking(false)
        .on_start(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.master().list_values().await?;
                dio.cache().insert_values(rows).await?;
                Ok(())
            }
        })
        .on_write(|dio, op| {
            let dio = dio.clone();
            async move {
                match &op {
                    WriteOp::Insert { id, record } | WriteOp::Replace { id, record } => {
                        dio.master().insert_value(id, record).await?;
                        dio.notify_record_changed(id.clone());
                    }
                    WriteOp::Delete { id } => {
                        dio.master().delete_value(id).await?;
                        dio.notify_record_changed(id.clone());
                    }
                    WriteOp::DeleteAll => {
                        dio.master().delete_all_values().await?;
                        dio.notify_dataset_changed();
                    }
                    _ => {}
                }
                Ok(())
            }
        })
        .refresh_every(Duration::from_secs(60))
        .build()?,
);

let products = lens.make_dio(remote_products).await?;
```

From here, `products.vista()` is a Vista. You use it the way you'd use any
other Vista:

```rust
// Read.
let all = products.vista().list_values().await?;
let one = products.vista().get_value(&"sku-1234".to_string()).await?;

// Narrow with eq conditions.
let mut filtered = products.vista();
filtered.add_condition_eq("category", "books".into())?;
let books = filtered.list_values().await?;

// Write.
products.vista().insert_value(&"sku-1234".to_string(), &new_product_record).await?;
products.vista().delete_value(&"sku-1234".to_string()).await?;

// Count.
let total = products.vista().get_count().await?;
```

If you're writing CLI code, this is functionally identical to what the
`bakery_model3/examples/cli-vista.rs` does today, except every read is
served from cache and every write invalidates the relevant rows.

## When you don't need Diorama

Plenty of Rust code shouldn't bother. Skip Diorama if:

- Your backing store is already fast (local sqlite, in-memory store) and
  you don't need offline tolerance.
- You're writing a one-shot tool — a migration script, a data export, a
  load test. The Lens's startup cost isn't justified.
- You only need a single operation per process invocation. A bare Vista is
  simpler.

The line is roughly: long-lived processes with read amplification or
write resilience requirements benefit; short-lived processes don't.

## Patterns by shape of work

### API endpoint backed by a slow remote

The example above. Cache reads, invalidate on write, short TTL. Lens lives
for the process lifetime; one Dio per resource type.

```rust
async fn list_products(state: State<AppState>) -> impl IntoResponse {
    let rows = state.products.vista().list_values().await?;
    Json(rows)
}

async fn update_product(state: State<AppState>, Path(id): Path<String>, Json(patch): Json<Patch>)
    -> impl IntoResponse
{
    state.products.vista().update(id, patch).await?;
    StatusCode::NO_CONTENT
}
```

The cache absorbs request bursts; the underlying remote sees a few requests
per minute instead of a few per second.

### Internal library that talks to multiple backends

You're building a library that other services call. It needs to read from
DynamoDB, write to Postgres, audit-log to S3. The user of your library
shouldn't have to think about the storage shape.

```rust
pub struct OrderService {
    orders: Dio,
}

impl OrderService {
    pub async fn new(lens: &Lens, db: DynamoClient, audit: S3Client) -> Result<Self> {
        let dynamo_vista = orders_dynamo_vista(db);
        let lens_with_audit = lens.clone().with_audit_target(audit);   // not real API, illustrative
        let orders = lens_with_audit.make_dio(dynamo_vista);
        Ok(Self { orders })
    }

    pub async fn create(&self, order: Order) -> Result<OrderId> {
        let id = self.orders.vista().insert(order.into_record()).await?;
        Ok(id)
    }

    pub async fn list_for_customer(&self, customer_id: &str) -> Result<Vec<Order>> {
        self.orders.vista()
            .add_condition_eq("customer_id", customer_id)
            .list_values_typed::<Order>()
            .await
    }
}
```

The Lens carries the strategy (cache to disk, write to Postgres + S3,
refresh on a timer). The service exposes typed business methods that don't
leak the storage shape. Consumers of `OrderService` don't see the Lens.

### Batch job with resumable state

A nightly job processes millions of records. It must survive restarts; it
must batch writes for throughput; it must avoid re-processing records seen
on a previous run.

```rust
let lens = Lens::new()
    .cache_at("./job-state.redb")
    .on_start(|dio| async move {
        let last_cursor: Option<String> = dio.cache().get_metadata("cursor").await?;
        let mut cursor = last_cursor.unwrap_or_default();
        loop {
            let page = dio.master().fetch_after(&cursor).await?;
            if page.is_empty() { break; }
            dio.cache().insert_values(page.rows.clone()).await?;
            cursor = page.next_cursor;
            dio.cache().set_metadata("cursor", &cursor).await?;
        }
        Ok(())
    })
    .build()
    .await?;

let job = lens.make_dio(source_vista);

// Process every record we have, then mark each as processed:
let pending = job.vista().add_condition_eq("processed", false).list_values().await?;
for row in pending {
    transform_and_emit(&row).await?;
    job.vista().update(row.id(), patch! { "processed": true }).await?;
}
```

On a clean run, `on_start` walks the cursor from zero and fills the cache.
On a restart, it picks up at `last_cursor`. The cache survives both. Writes
go through the local cache instantly and the background worker batches them
to the master.

### CLI tool with persistent local state

The same `bakery_model3/examples/cli-vista.rs` pattern, but with a local
cache that survives between command invocations:

```rust
let lens = Lens::new()
    .cache_at(dirs::cache_dir().unwrap().join("mytool.redb"))
    .on_start(|dio| async move {
        // Only fetch if cache is empty.
        if dio.cache().count().await? == 0 {
            let data = dio.master().list_values().await?;
            dio.cache().insert_values(data).await?;
        }
        Ok(())
    })
    .build()
    .await?;

let products = lens.make_dio(api_vista);
println!("{:?}", products.vista().list_values().await?);
```

Second invocation: instant output. Add an explicit `--refresh` flag that
calls `products.refresh().await?` to force a re-fetch when staleness matters.

## Don't reach for Sceneries

If you're writing business logic, you don't need them. Sceneries are for UI
adapters and other consumers that pull-on-render. CLI code, API handlers,
job runners, internal libraries — all of them want the snapshot API
(`vista().list_values().await`, `vista().get_value(id).await`).

If you find yourself thinking "I want to know when this data changes" in
business code, you usually want either:

- An `on_event` callback in the Lens (the change comes from an external
  source you can subscribe to).
- A polling loop with an explicit interval.
- A websocket or queue subscription unrelated to Diorama.

Sceneries exist because UI frameworks have specific pull-on-render
contracts. Business code doesn't share that constraint.

## Error handling

`dio.vista().list_values()` returns `Result<Vec<Record<CborValue>>, Error>`.
The error surface is the same as plain Vista:

- The cache failed (disk full, redb corrupted, in-memory backend gone).
- A write was rejected by the cache.
- An `on_query` callback failed and the cache is cold.

What you don't see in the synchronous return:

- Asynchronous write failures. Writes through `dio.vista()` enqueue and
  return `Ok(())` immediately. If the queued write later fails (master
  rejected it, network died), the failure is published as a `DioEvent` on
  the event bus. Business code that cares about write durability needs to
  subscribe to the event bus or use a `Lens::on_write` callback that calls
  master synchronously and propagates the error.
- Refresh failures. Background refreshes fail silently from the caller's
  perspective; the cache keeps serving its last good data. Wire a
  `tracing` subscriber to see the errors during development.

For workloads that need strict write-then-confirm semantics — financial
transactions, regulatory data — write a synchronous `on_write` that awaits
the master and returns the result. The framework gets out of your way.

## Composition with other Vistas

You don't have to wrap a single Vista. `lens.make_dio` takes anything that
implements the Vista interface, including composed Vistas. The composition
primitives live in this same crate:

```rust
use vantage_diorama::Diorama;

// Overlay: read from CSV, write to memory, capabilities are the union.
let overlay = Diorama::overlay(read_only_csv_vista, in_memory_write_vista);
let dio = lens.make_dio(overlay);

// Merge: read from two sources, prefer the first.
let merged = Diorama::merge(local_cache_vista, remote_api_vista);
let dio = lens.make_dio(merged);
```

Composition produces something that satisfies the Vista interface; the Lens
treats it indistinguishably from a leaf Vista. The composed Vista's
capabilities are the union of its parts, which the Lens then re-derives one
more time before producing `dio.vista()`.

For most business code, you don't need composition. It's primarily for
scenarios where the data shape genuinely lives across two sources — a
read-only seed file plus user-edited overlay, a primary database plus a
fallback cache, a federated query across two systems. Reach for it when the
shape demands it, not preemptively.

## A complete worked example

A REST API service that surfaces a slow remote product catalog with a
read cache and write-through.

```rust
use std::sync::Arc;
use std::time::Duration;
use axum::{routing::{get, post}, Router, extract::State, Json};
use vantage_diorama::{Dio, Lens};

#[derive(Clone)]
struct AppState {
    products: Dio,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let remote = RemoteCatalogVista::new(std::env::var("CATALOG_URL")?).into_vista()?;

    let lens = Lens::new()
        .cache_at("./catalog.redb")
        .on_start(|dio| async move {
            let data = dio.master().list_values().await?;
            dio.cache().insert_values(data).await?;
            Ok(())
        })
        .on_refresh(|dio| async move {
            let fresh = dio.master().list_values().await?;
            dio.cache().replace_all(fresh).await?;
            Ok(())
        })
        .on_write(|dio, op| async move {
            dio.master().apply(&op).await?;
            if let Some(id) = op.target_id() {
                dio.notify_record_changed(id);
            }
            Ok(())
        })
        .refresh_every(Duration::from_secs(300))
        .build()
        .await?;

    let products = lens.make_dio(remote);
    let state = AppState { products };

    let app = Router::new()
        .route("/products", get(list_products))
        .route("/products/:id", get(get_product).put(update_product))
        .with_state(state);

    axum::Server::bind(&"0.0.0.0:3000".parse()?).serve(app.into_make_service()).await?;
    Ok(())
}

async fn list_products(State(state): State<AppState>) -> Result<Json<Vec<Product>>, AppError> {
    let rows = state.products.vista().list_values_typed::<Product>().await?;
    Ok(Json(rows))
}

async fn get_product(State(state): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>)
    -> Result<Json<Product>, AppError>
{
    let row = state.products.vista().get_value_typed::<Product>(&id).await?;
    row.map(Json).ok_or(AppError::NotFound)
}

async fn update_product(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(patch): Json<ProductPatch>,
) -> Result<axum::http::StatusCode, AppError> {
    state.products.vista().update(id, patch.into_record()).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
```

The service starts up by loading the catalog once. Reads hit the disk cache.
Writes go to the cache and the master synchronously, so the client knows the
write succeeded before the response returns. Background refresh keeps the
cache fresh against upstream changes.

No reactive code. No Sceneries. No UI concerns. Diorama just made the slow
backend usable.

## Inspecting a live Dio

`dio.diagnostics().await` snapshots what a Dio is doing right now — handy for a
debug panel, a `/healthz`-style endpoint, or a test assertion:

```rust
let d = dio.diagnostics().await;
println!("cache: {} rows, {} query indexes", d.cache_rows, d.query_indexes);
for s in &d.sceneries {
    // key = (shape, conditions, sort, search, titles_only); refcount = widgets holding it
    println!(
        "{}  x{}  rows={}  fresh={}/{} pending={} failed={}",
        s.key, s.refcount, s.row_count,
        s.status.fresh, s.status.loaded, s.status.pending_write, s.status.failed,
    );
}
println!("augmented rows on screen: {}", d.augmented_rows());
```

It reads straight off the dedup registry, so it's nearly free and prunes dead
entries as it goes — a released scenery disappears from the report, which is
exactly how you confirm nothing leaked. `dio.live_table_scenery_count()` is the
one-number version.
