# Vantage Diorama

A Vista is a window into a single data source — a CSV file, a DynamoDB table, a
GraphQL endpoint, a SurrealDB collection. Each source has its quirks: some can't
sort, some are slow, some go offline. Diorama turns one or many such Vistas into
something your application can actually use: a cached, composable, reactive
surface that hides the rough edges.

Three things happen in this crate:

1. **Caching.** Diorama keeps a local copy of data behind a Vista. The copy can
   live in memory, on disk, or anywhere else you point it. Reads come from the
   cache; writes go through it; refreshes are scheduled.
2. **Composition.** When one Vista can't do everything you need — read-only CSV,
   write-only audit log, no sort, no filter — you stack Vistas together. The
   result advertises the union of their capabilities.
3. **Reactivity.** When data changes, anything watching the Diorama hears about
   it. A grid widget re-renders. A form sheet updates. A counter in your menu
   bar refreshes. You write the data flow once; the UI follows.

## The four words

You'll see these throughout the docs. Worth pinning down up front.

- **Vista** — a single backend data source. Defined by `vantage-vista`. Speaks
  whatever capabilities the backend natively supports.
- **Lens** — the long-lived apparatus that holds your caching strategy, cache
  backend, and lifecycle callbacks. Configured once per application.
- **Dio** (short for *Diorama*) — the result of running a Vista through a Lens.
  Produced cheaply by `lens.make_dio(vista)`. Owns the wrapped data + the
  per-entity machinery.
- **Scenery** — a reactive view onto a Dio. Tables, single records, and single
  values all have their own Scenery type. The UI binds to it.

The picture: `Vista → Lens.make_dio(vista) → Dio → Vista | Scenery`.

## A small example

```rust
use vantage_diorama::Lens;
use bakery_model3::Product;

let lens = Lens::new()
    .cache_at("./local.redb")
    .on_start(|dio| async move {
        let data = dio.master().list_values().await?;
        dio.cache().insert_values(data).await?;
        Ok(())
    })
    .refresh_every(std::time::Duration::from_secs(3600))
    .build()
    .await?;

let products = lens.make_dio(Product::dynamo_table(db).into_vista()?);

// Use it like any other Vista — but reads are instant, served from cache:
let cheap = products.vista()
    .where_eq("price_max", 10)
    .list_values()
    .await?;

// Or open a reactive view that re-renders when data changes:
let scenery = products.table_scenery()
    .sort("price", Asc)
    .open();
```

The Lens is the hard part: it decides what to cache, when to refresh, how to
route writes. You write it once. After that, every Dio you make from it inherits
the same policy, and the surfaces you spawn from each Dio (Vistas, Sceneries)
are cheap to produce and cheap to drop.

## Where to go next

Pick the role that matches what you're trying to do.

- [Writing business logic](README_rust_dev.md) — using Diorama from API
  handlers, internal libraries, CLIs. No UI involved.
- [Configuring a Lens](README_lens.md) — cache strategies, callback patterns,
  real-life scenarios beyond UI (mobile, edge, server-side).
- [Wiring up a UI](README_ui.md) — binding Scenery to GPUI widgets and writing
  adapters for other frameworks.
- [Defining models](README_models.md) — what column metadata helps Diorama and
  what to copy from `bakery_model3`.
- [Implementing a data source](README_datasource.md) — what your Vista driver
  has to do to be a good Diorama citizen.
- [Architecture](ARCHITECTURE.md) — protocols, trait surfaces, concurrency
  model. For maintainers and adapter authors.

## Status

This crate is in design. The READMEs describe the intended API; types and
implementations land in `vantage-diorama/src/` as the plan progresses. See
[`vantage-vista/plans/`](../vantage-vista/plans/) for the broader vista
roadmap that this work sits inside.
