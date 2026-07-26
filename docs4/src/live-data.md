# Live Data: Push, Poll & Freshness

A cache only stays correct if it finds out when the underlying data changes. There are two ways it
can: it asks (**poll**), or the database tells it (**push**). `dio.watch()` is the same call either
way — it asks the master Vista whether it can push, subscribes if so, and quietly does nothing if
not.

That last part is the bit worth knowing before you pick a backend. `watch()` returning `Ok(())` does
not mean you have a live feed; it means nothing went wrong. This page says exactly what each shipped
driver delivers today, and what — if anything — you have to do to get it.

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

## What each backend delivers today

| Backend        | Mechanism                              | Model       | Setup required                          |
| -------------- | -------------------------------------- | ----------- | --------------------------------------- |
| SurrealDB      | `LIVE SELECT` over WebSocket           | fine push   | none                                    |
| PostgreSQL     | `LISTEN/NOTIFY` on `{table}_changed`   | coarse push | a trigger **and** an explicit opt-in    |
| SQLite         | —                                      | poll        | —                                       |
| MySQL          | —                                      | poll        | —                                       |
| MongoDB        | —                                      | poll        | —                                       |
| CSV files      | —                                      | poll        | —                                       |
| REST / GraphQL | —                                      | poll        | —                                       |
| AWS            | —                                      | poll        | —                                       |
| Kubernetes     | —                                      | poll        | —                                       |
| CLI tools      | —                                      | poll        | —                                       |
| Append logs    | — (write-only sink)                    | n/a         | —                                       |

Only two drivers implement `watch_vista` at all. Everything in the poll rows inherits the trait
default, which reports the capability as unsupported and leaves
[`can_subscribe`](vantage_vista::VistaCapabilities) `false` — so `dio.watch()` on those is a no-op
and a `refresh_every` timer carries the load.

```admonish note title="Poll is a supported answer, not a failure"
A polled backend is not broken, and you do not need different application code for it. The Lens keeps
its `refresh_every`, the scenery is content-aware, and an unchanged re-read bumps no generation and
repaints nothing. The cost is latency and a query you didn't strictly need — not correctness.
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
from a table where nothing happens. So it is opt-in:

```rust
let master = db
    .vista_factory()
    .with_notify(true)   // yes, I installed the trigger
    .from_table(Product::table(db.clone()))?;
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
  driver reconciles or you must.
- **An invalidation means "re-read everything."** It carries no id and implies nothing about how much
  changed.
- **The stream ending is normal.** Connections drop; consumers resubscribe with backoff and reconcile
  the gap. `Dio::watch()` already does this for you.

The consequence worth internalising: [`can_subscribe`](vantage_vista::VistaCapabilities) advertises
an *attempt* to push, not a guarantee of delivery. No shipped backend is lossless, so anything that
must not miss a change keeps a reconcile path — a slow timer, a refresh on reconnect — rather than
trusting push alone.

## Adding push to a driver

If you are implementing a backend, the driver-side contract — what you must promise when you
override `watch_vista`, and when you may honestly advertise the capability — is step 8 of
[Adding a New Persistence](./new-persistence/step8-vista-integration.md). Start with the capability
`false`; turning it on the day the feed works is a healthier state than shipping a flag that lies.
