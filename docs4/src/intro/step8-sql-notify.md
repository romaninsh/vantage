# Real-Time Push with LISTEN/NOTIFY

The app so far reconciles on a **timer** — `Lens::refresh_every(Duration::from_secs(1))`. That is
honest polling: once a second the Dio re-reads the master and diffs. It works, but it is a beat
behind every change and it re-queries even when nothing moved. A file-backed SQLite database has no
better option — it can't call you back. A PostgreSQL *server* can: it can tell you the instant a row
changes, and then the poll disappears.

This chapter does exactly that. Having proven the app is portable, we **commit to Postgres** — drop
the feature flag, delete `sim.rs`, and split the writer into its own process — and replace the
refresh timer with a single `dio.watch()`. The finished crate is
[`learn-10`](https://github.com/romaninsh/vantage/tree/main/learn-10).

## Teach the database to announce changes

Postgres `LISTEN/NOTIFY` is a publish/subscribe channel built into the server. A trigger fires
`pg_notify` on every write; anyone `LISTEN`ing on that channel wakes up. So the only schema we add
beyond the table is a trigger:

```rust
// src/db.rs — after CREATE TABLE product (...)
sqlx::query(
    "CREATE OR REPLACE FUNCTION product_notify() RETURNS trigger AS $$
     BEGIN PERFORM pg_notify('product_changed', ''); RETURN NULL; END;
     $$ LANGUAGE plpgsql",
).execute(db.pool()).await?;

sqlx::query(
    "CREATE TRIGGER product_notify_trg
     AFTER INSERT OR UPDATE OR DELETE ON product
     FOR EACH STATEMENT EXECUTE FUNCTION product_notify()",
).execute(db.pool()).await?;
```

`FOR EACH STATEMENT` fires the notify once per statement, not per row — a single "something
changed" ping is all the Dio needs to reconcile. The payload is empty on purpose (more on that
below).

## One line replaces the poll

The server builds the Dio exactly as before — an eager in-memory cache, an `on_start` that loads it
and an `on_refresh` that reloads it — but with **no** `refresh_every`. Instead:

```rust
let dio = lens.make_dio(master).await?;

// The transparent live feed. The master Vista advertises `can_watch`, so this
// subscribes over LISTEN/NOTIFY and reconciles the instant a write lands — no
// polling timer.
dio.watch().await?;
```

That is the whole change. [`Dio::watch()`](https://docs.rs/vantage-diorama) asks the master Vista
whether it can push (`Vista::can_watch()`); the Postgres Vista answers yes and opens a `PgListener`
on the `product_changed` channel. Each notification drives one reconcile. If the backend *couldn't*
push, `watch()` would be a no-op and a `refresh_every` timer would carry the load instead — the same
call is correct either way, which is the point of the capability.

```admonish note title="The server still never writes"
`dio.watch()` only *reads and reconciles*. Nothing in the server process writes to `product`. That
separation is what makes the demo honest: whatever you see on screen arrived over the database.
```

## The till becomes its own process

To drive it, `learn-10` ships a second binary — the **mutator** — run alongside the server:

```text
$ cargo run -p learn-10                 # the reactive server, port 3010
$ cargo run -p learn-10 --bin mutator   # a separate writer process
```

The mutator writes through Vantage's **active-entity** API — no hand-written SQL. `list_entities()`
hands back drinks that each carry their own id and datasource, so selling one is just:

```rust
let mut shelf = table.list_entities().await?;
let drink = shelf.choose_mut(&mut rand::thread_rng()).unwrap();
if drink.stock <= 1 {
    drink.delete().await?;        // last unit — off the shelf
} else {
    drink.stock -= 1;
    drink.save().await?;          // one sale
}
```

Two independent processes that share nothing but the database and the notify channel: the strongest
possible proof that the reactive stack tracks the *data*, not some in-process shortcut.

## Removals, and a UI to see them

This app also keys its watch by identity, so a sold-out drink is reported as a real removal rather
than a silent list-shrink:

```rust
let api = DioRouter::new(dio.clone())
    .with_column("id", "id")
    .with_column("name", "name")
    .with_column("price", "price")
    .with_column("stock", "stock")
    .key_by("id")            // identity-keyed: emits DELETED when a row leaves
    .with_page_size(50)
    .into_router();
```

With `key_by("id")`, the watch stream gains a third event — `DELETED` — alongside `ADDED` and
`MODIFIED`. A small React page (`frontend/index.html`, served by the same axum app) consumes the
stream and animates each drink: arriving with a flash, ticking down as it sells, and *disposing*
with a "sold out" stamp when a `DELETED` lands. It is plain React over the watch endpoint — not part
of Vantage — and, notably, the **exact same file** the next chapter reuses unchanged.

Watch it on the wire:

```text
$ curl -N 'localhost:3010/api/products?watch=true'
{"type":"ADDED","object":{"index":0,"id":"d51...","name":"Mojito","price":1000,"stock":11}}
{"type":"ADDED","object":{"index":1,"id":"d59...","name":"Boulevardier","price":1400,"stock":12}}
{"type":"MODIFIED","object":{"index":0,"id":"d51...","name":"Mojito","price":1000,"stock":10}}
{"type":"DELETED","object":{"index":0,"id":"d59...","name":"Boulevardier","price":1400,"stock":1}}
```

Every line is a real database write, announced by Postgres, reconciled into the cache, diffed by the
scenery, and fanned out — with no timer anywhere in the loop.

```admonish tip title="Coarse push: a ping, then a re-read"
`NOTIFY` carries no row data — the payload is empty. So `dio.watch()` learns only *that* something
changed and responds by re-reading the whole set to reconcile. That is perfectly correct and, for a
shelf of a few dozen drinks, instant. But it is a **coarse** signal: one write means one full
reconcile. Hold that thought — the next chapter switches to a database whose push carries the
changed row itself, so no re-query is needed at all.
```

Next: the same app, the same frontend, a different database — SurrealDB, whose native live queries
push the actual change rather than a ping.
