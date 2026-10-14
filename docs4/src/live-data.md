# Live Data: Push, Poll & Freshness

A cache only stays correct if it finds out when the underlying data changes. There are two ways it
can: it asks (**poll**), or the database tells it (**push**). A Dio asks for push with one call on
its builder, `DioBuilder::live()`: the Dio checks whether the master Vista advertises
[`Capability::Subscribe`](vantage_vista::Capability), follows its change stream if so, and quietly
does nothing if not.

That last part is the bit worth knowing before you pick a backend. A `live()` Dio that builds
without error does not mean you have a live feed; it means nothing went wrong. This page says
exactly what each shipped driver delivers today, what — if anything — you have to do to get it, and
how the Lens keeps a cache fresh when push isn't there.

<!-- toc -->

---

## Three freshness models

Every backend answers two questions, and the pair decides what you get:

1. **Can it tell you something changed at all?** A file on disk cannot. A database server can.
2. **When it does, does it hand you the row, or only a signal?**

| Model          | What arrives                                | What the Dio does                     |
| -------------- | ------------------------------------------- | ------------------------------------- |
| **poll**       | nothing — you re-read on a timer            | re-lists the whole set                |
| **coarse push** | "something changed", no payload             | re-reads the set to find out what     |
| **fine push**  | the action *and* the affected record        | applies that one row to the cache     |

Fine push is the only model where a change costs you no query. Coarse push still beats polling: you
re-read *because something happened*, not on the off-chance.

There is a fourth position, currently held by one backend. Fine push still leaves a question open —
*does this row belong in my filtered set?* — because a driver that watches the whole table will hand
you rows your conditions exclude. **Filtered fine push** closes it: the conditions go into the
subscription itself and the database maintains the result incrementally, so a row that changes *out*
of the set arrives as a delete. Membership stops being something anyone has to reconcile.

## What each backend delivers today

| Backend        | Mechanism                              | Model       | Setup required                          |
| -------------- | -------------------------------------- | ----------- | --------------------------------------- |
| SpacetimeDB    | subscription over WebSocket (BSATN)    | filtered fine push | none                             |
| SurrealDB      | `LIVE SELECT` over WebSocket           | fine push   | none                                    |
| PostgreSQL     | `LISTEN/NOTIFY` on `{table}_changed`   | coarse push | a trigger **and** an explicit opt-in    |
| SQLite         | —                                      | poll        | —                                       |
| MySQL          | —                                      | poll        | —                                       |
| MongoDB        | change streams                         | fine push   | a replica set or sharded cluster        |
| CSV files      | —                                      | poll        | —                                       |
| REST / GraphQL | —                                      | poll        | —                                       |
| AWS            | —                                      | poll        | —                                       |
| Kubernetes     | —                                      | poll        | —                                       |
| CLI tools      | —                                      | poll        | —                                       |
| Append logs    | — (write-only sink)                    | n/a         | —                                       |

MySQL is poll-only: it has no `LISTEN/NOTIFY` equivalent, and pushing changes would need a binlog
replication client.

Only four drivers implement `TableShell::watch` at all. A standalone MongoDB server has no change
streams and polls. `MongoVistaFactory::with_change_streams(true)` is for overriding a failed
(`Unknown`) topology probe, say behind a proxy that refuses `hello`; it cannot give a standalone
server change streams, and forcing it on there makes the watch fail on the server. Everything in the
poll rows inherits the trait default, which refuses `watch` and leaves `Capability::Subscribe` out
of the shell's capabilities — so `live()` on those does nothing and the Lens's refresher carries the
load.

```admonish note title="Poll is a supported answer, not a failure"
A polled backend is not broken, and you do not need different application code for it. The Lens
keeps its refresher, the scenery is content-aware, and an unchanged re-read bumps no generation and
repaints nothing. The cost is latency and a query you didn't strictly need — not correctness.
```

## Polling: the Lens's refresher

A Lens re-reads its Dios through a [`Refresher`](vantage_diorama::Refresher). Its function gets a
`LoaderCx` and does the re-read; `LoaderCx::reconcile` makes the cache hold exactly the snapshot it
is given. Two settings decide when it runs:

- `Refresher::every(interval)` runs it on a timer, `interval` after each Dio is built and then
  every `interval`.
- `Refresher::max_age(ttl)` bounds how stale a cache may be when it is opened. A Dio whose last
  successful refresh is older than `ttl`, or unrecorded, gets a background refresh when it is built
  and whenever a table or record scenery opens on it. The refresh time is stored in the cache, so
  it holds across restarts.

They combine freely. `max_age` alone runs no timer: a cache that was refreshed five minutes ago and
opened again is left alone until it is older than its bound. `every` alone keeps refreshing whether
anyone opens the cache or not. Both together is the usual setting for a polled backend behind a
persistent cache: a refresh on open after a long absence, then a steady timer while the
application runs.

```rust
# use std::time::Duration;
#
# use ciborium::Value as CborValue;
# use vantage_dataset::ReadableValueSet;
# use vantage_diorama::{Cache, DioEvent, Lens, Loader, Refresher};
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::{Column, Vista, VistaMetadata};
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let mut lobby = Record::new();
# lobby.insert("name".to_string(), CborValue::Text("Lobby".into()));
# let rooms = Vista::new(
#     "rooms",
#     MockShell::new()
#         .with_metadata(
#             VistaMetadata::new()
#                 .with_column(Column::new("id", "String").with_flag("id"))
#                 .with_column(Column::new("name", "String"))
#                 .with_id_column("id"),
#         )
#         .with_record("r1", lobby),
# );
let lens = Lens::builder()
    .cache(Cache::memory())
    .loader(Loader::eager(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    }))
    .refresher(
        Refresher::new(|cx| async move {
            let rows = cx.master().list_values().await?;
            cx.reconcile(rows).await
        })
        .max_age(Duration::from_secs(15 * 60))
        .every(Duration::from_secs(60)),
    )
    .build()?;

// Follows the master's change stream when it advertises Subscribe;
// over a polled master the refresher above keeps the cache fresh instead.
let dio = lens.dio(rooms).live().build().await?;

let mut events = dio.subscribe_events();
tokio::spawn(async move {
    while let Ok(event) = events.recv().await {
        if let DioEvent::RefreshFailed { error, .. } = event {
            eprintln!("rooms may be stale: {error}");
        }
    }
});
# assert_eq!(dio.cached().list_values().await?.len(), 1);
# Ok(())
# }
```

Without a refresher a Lens has no timer and no staleness check; the cache holds what the loader and
the writes put there.

### Why not refresh on every open?

Because for a persistent cache, "on every open" is "on every screen change". A grid that the user
leaves and comes back to would re-list its whole set each time, which on a slow REST source is the
latency the cache was there to hide. `max_age` keeps the refresh where it pays: after an absence
long enough that the data has plausibly moved.

## Changes you feed in yourself

A source with its own change feed that no driver wraps — a webhook, a message queue, a file
watcher — feeds the Dio directly. Two calls cover it:

- `Dio::apply_change(ChangeEvent)` applies one change: `Inserted` and `Updated` upsert the row,
  `Deleted` removes it, and `Invalidated` re-reads the whole set. Each publishes the matching
  `DioEvent`, so open sceneries repaint.
- `Dio::reconcile(snapshot)` makes the cache hold exactly a full snapshot: rows absent from it are
  deleted, except one with a write in flight, and one `DatasetChanged` is published.

```rust
# use ciborium::Value as CborValue;
# use vantage_dataset::ReadableValueSet;
# use vantage_diorama::{Cache, ChangeEvent, Lens};
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::{Column, Vista, VistaMetadata};
#
# fn room(name: &str) -> Record<CborValue> {
#     let mut r = Record::new();
#     r.insert("name".to_string(), CborValue::Text(name.to_string()));
#     r
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let rooms = Vista::new(
#     "rooms",
#     MockShell::new().with_metadata(
#         VistaMetadata::new()
#             .with_column(Column::new("id", "String").with_flag("id"))
#             .with_column(Column::new("name", "String"))
#             .with_id_column("id"),
#     ),
# );
# let lens = Lens::builder().cache(Cache::memory()).build()?;
let dio = lens.dio(rooms).build().await?;

dio.apply_change(ChangeEvent::Inserted { id: "r1".into(), new: Some(room("Lobby")) }).await?;
dio.apply_change(ChangeEvent::Updated { id: "r1".into(), new: Some(room("Atrium")) }).await?;

let snapshot = [("r2".to_string(), room("Kitchen"))].into_iter().collect();
dio.reconcile(snapshot).await?;

let cached = dio.cached().list_values().await?;
assert!(cached.len() == 1 && cached.contains_key("r2"));
# Ok(())
# }
```

A `None` row on `Inserted` or `Updated` publishes the event without touching the cache: the
notify-only shape a coarse feed produces. A refresher's or loader's own re-read goes through
`LoaderCx::reconcile` instead, which also leaves alone any row whose write settled while the read
was in flight.

## SpacetimeDB — filtered fine push, no setup

SpacetimeDB is a database that is also the application server: a WASM module holding the schema and
the reducers runs inside it, and the client's normal way to read anything is to subscribe. Push is
not a feature bolted on beside queries; it is the product.

The driver sends the Vista's own conditions as the subscription query, so the server evaluates them
and only matching rows are ever transmitted. Because it maintains that result incrementally, a row
that changes so it *no longer* matches arrives as a delete — you are told the row left, not merely
that something happened. That is why this is the only row in the table where set membership needs no
reconciliation from anyone: not a re-read per notification as SurrealDB does, and not a whole-set
reload as Postgres does.

Nothing needs installing or enabling. There is no trigger, no extension, no opt-in flag — which is
why its shells simply advertise `Subscribe` rather than something you have to declare.

```admonish note title="This backend gives you less everywhere else"
The trade is real and worth stating. SpacetimeDB's SQL has no `ORDER BY`, `OFFSET`, `GROUP BY`, `IN`
or `LIKE`, and the driver does not emulate them — a grid over a SpacetimeDB table shows rows in the
server's natural order and its sort headers are inert. Aggregates for a chart have to be materialised
by the module into a rollup table or computed in a view. You are choosing a backend that pushes
beautifully and queries narrowly.
```

## SurrealDB — fine push, no setup

`LIVE SELECT` delivers a typed per-row notification: the action (`CREATE` / `UPDATE` / `DELETE`) and
the affected record. The driver re-reads each notified id *through the vista's own conditions*, so a
row that no longer belongs to a filtered set arrives as a delete rather than silently lingering. A
real table is watchable the moment you build it; only query-sourced vistas (`rhai:` / `base:`) are
not.

```admonish note title="Live queries ride the WebSocket connection"
`LIVE SELECT` needs a transport the server can push frames down, which means WebSocket. That is not a
trap you can fall into: `SurrealConnection::connect` accepts only `ws://`, `wss://` and `cbor://` and
rejects anything else outright, so every SurrealDB connection you can actually open is one that can
carry live queries.
```

## PostgreSQL — coarse push, and why you must ask for it

Postgres has no built-in change feed. It has `LISTEN/NOTIFY`, a general-purpose pub/sub channel, and
a trigger you install yourself decides what gets announced. The driver listens on `{table}_changed`
and yields a payload-less invalidation for every notification.

Because you install that trigger, only you know whether it exists — and `LISTEN` on a channel nobody
ever feeds *succeeds*, then blocks forever. A Vista that assumed it could push merely because the
table is writable would advertise a feed that may never arrive, which a consumer cannot tell apart
from a table where nothing happens. So it is opt-in. The block needs a running Postgres, so it is
compiled but not run:

```rust,no_run
# use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
# use vantage_table::table::Table;
# use vantage_types::entity;
# use vantage_core::Context;
#
# #[entity(PostgresType)]
# #[derive(Debug, Clone, Default)]
# pub struct Product {
#     pub name: String,
# }
#
# impl Product {
#     pub fn table(db: PostgresDB) -> Table<PostgresDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = PostgresDB::connect("postgres://localhost/bakery").await.context("connect")?;
let master = db
    .vista_factory()
    .with_notify(true)   // yes, I installed the trigger
    .from_table(Product::table(db.clone()))?;
# let _ = master;
# Ok(())
# }
```

The trigger itself, and the full walkthrough, are in
[Real-Time Push with LISTEN/NOTIFY](./intro/step8-sql-notify.md). The declaration applies to every
table a factory builds, so if only some of your tables carry triggers, use separate factories. It
also carries across reference traversal — a relation target inherits the parent's opt-in rather than
silently reverting to unwatchable.

## What a consumer must assume

Push is best-effort, and the contract is deliberately loose so that a driver can tighten its scope
later without breaking anyone. Callers always hand over the full Vista; the driver delivers what it
can. Four things hold everywhere:

- **The stream may be coarser than your Vista.** Watching the whole table and letting the consumer
  discard the rest is a legitimate implementation.
- **A pushed row may fall outside your conditions**, precisely because of the above. Either the
  driver reconciles or you must. A live Dio holds each change against its own query, and applies a
  row that falls outside it as a delete.
- **An invalidation means "re-read everything."** It carries no id and implies nothing about how much
  changed.
- **The stream ending is normal.** Connections drop; consumers resubscribe with backoff and reconcile
  the gap. A `live()` Dio already does this: it re-subscribes, re-reads the set, and publishes
  `DioEvent::RefreshFailed` for each re-read that fails until one succeeds. The sceneries keep the
  rows they show meanwhile.

The consequence worth internalising: `Capability::Subscribe` advertises an *attempt* to push, not a
guarantee of delivery. No shipped backend is lossless, so anything that must not miss a change keeps
a reconcile path — a refresher with `every`, a refresh on reconnect — rather than trusting push
alone.

## Adding push to a driver

If you are implementing a backend, the driver-side contract — what you must promise when you
implement `watch`, and when you may honestly advertise `Subscribe` — is step 8 of
[Adding a New Persistence](./new-persistence/step8-vista-integration.md). Start without the
capability; turning it on the day the feed works is a healthier state than shipping a flag that
lies.

## Checklist

To keep a Dio fresh:

1. **Know your backend's model** — the table above; push needs `Capability::Subscribe` on the
   master.
2. **Ask for push** — `lens.dio(master).live().build()`. Over a polled master it does nothing.
3. **Give the Lens a refresher** — `Refresher::new(..)` with `max_age` for staleness on open,
   `every` for a timer, or both.
4. **Feed changes no driver wraps** — `dio.apply_change(..)` per change, `dio.reconcile(..)` for a
   full snapshot.
5. **Watch for `DioEvent::RefreshFailed`** — the cache may be out of step until a retry succeeds.
