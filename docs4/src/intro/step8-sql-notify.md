# Real-Time Push with LISTEN/NOTIFY

The app so far reconciles on a **timer**: `Refresher::new(reload).every(Duration::from_secs(1))`.
That is polling. Once a second the Dio re-reads the master and reconciles. It works, but it is a
beat behind every change, and it re-queries even when nothing moved. A file-backed SQLite database
has no better option, because it can't call you back. A PostgreSQL *server* can: it can tell you
the instant a row changes, and then the poll goes away.

Having shown the app is portable, this chapter **commits to Postgres**: it drops the feature flag,
deletes `sim.rs`, moves the writer into its own process, and replaces the refresher with one call
on the Dio builder, `.live()`. The finished crate is
[`learn-10`](https://github.com/romaninsh/vantage/tree/main/learn-10).

## Teach the database to announce changes

Postgres `LISTEN/NOTIFY` is a publish/subscribe channel built into the server. A trigger calls
`pg_notify` on every write, and anyone `LISTEN`ing on that channel wakes up. So the only schema
added beyond the table is a trigger. `db::setup` sends these statements, each through
`sqlx::query(..).execute(db.pool())`, after `CREATE TABLE product (..)`:

```sql
CREATE OR REPLACE FUNCTION product_notify() RETURNS trigger AS $$
BEGIN PERFORM pg_notify('product_changed', ''); RETURN NULL; END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS product_notify_trg ON product;

CREATE TRIGGER product_notify_trg
AFTER INSERT OR UPDATE OR DELETE ON product
FOR EACH STATEMENT EXECUTE FUNCTION product_notify();
```

`FOR EACH STATEMENT` fires the notify once per statement, not per row: a single "something
changed" ping is all the Dio needs to reconcile. The payload is empty on purpose (more on that
below). The channel is `product_changed` because the Postgres Vista listens on `{table}_changed`.

## Tell the Vista the trigger exists

Postgres offers no way to ask "is anything going to notify me on this channel?". A `LISTEN` on a
channel nobody feeds succeeds and then waits forever. A Vista that assumed it could push because
the table is writable would advertise a feed that may never arrive, and no consumer could tell that
apart from a quiet table. So the application, the only party that knows, says so:

```rust,no_run
# use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# use vantage_vista::SortDirection;
#
# #[entity(PostgresType)]
# #[derive(Debug, Clone, Default)]
# pub struct Product {
#     pub name: String,
#     pub price: i64,
#     pub stock: i64,
#     pub created: i64,
# }
#
# impl Product {
#     pub fn table(db: PostgresDB) -> Table<PostgresDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("stock")
#             .with_column_of::<i64>("created")
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
# let db = PostgresDB::connect(&url).await.context("connect postgres")?;
let mut master = db
    .vista_factory()
    .with_notify(true) // db::setup installed product_notify_trg
    .from_table(Product::table(db.clone()))?;
master.set_order(Some(("created", SortDirection::Ascending)))?;
# Ok(())
# }
```

`with_notify(true)` is what lets the Vista advertise
[`Subscribe`](vantage_vista::Capability::Subscribe). Leave it off and the live follow below does
nothing. That matters here, because this chapter deletes the refresher: push and a refresher's
timer are the two ways the cache learns about a write from outside, and an app that declines both
never updates.

The model gained a `created` column, an epoch-millis stamp set on delivery. The shelf is ordered by
it, so a drink keeps its place as it sells and new deliveries append at the end.

## One call replaces the poll

The server builds the Lens as before, with an eager in-memory cache and a loader that reconciles,
but with **no** refresher. The Dio builder gets `.live()` instead:

```rust,no_run
# use vantage_diorama::prelude::*;
# use vantage_diorama::{Cache, Loader, LoaderCx};
# use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
#
# #[entity(PostgresType)]
# #[derive(Debug, Clone, Default)]
# pub struct Product {
#     pub name: String,
#     pub price: i64,
#     pub stock: i64,
#     pub created: i64,
# }
#
# impl Product {
#     pub fn table(db: PostgresDB) -> Table<PostgresDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("stock")
#             .with_column_of::<i64>("created")
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
# let db = PostgresDB::connect(&url).await.context("connect postgres")?;
# let master = db
#     .vista_factory()
#     .with_notify(true)
#     .from_table(Product::table(db.clone()))?;
let lens = Lens::builder()
    .cache(Cache::memory())
    .loader(Loader::eager(|cx: LoaderCx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    }))
    .build()
    .context("build lens")?;

let dio = lens.dio(master).live().build().await?;
# Ok(())
# }
```

That is the whole change. [`live()`](vantage_diorama::DioBuilder::live) checks whether the master
advertises `Subscribe`. Because we opted in above, the Postgres Vista does, and the Dio opens a
`LISTEN` on `product_changed`. Each notification arrives as a coarse change, "the set changed", and
the Dio answers it with a [refresh](vantage_diorama::Dio::refresh). With no refresher on the Lens,
a refresh re-runs the eager loader, so the same `cx.reconcile(rows)` that warmed the cache brings
it up to date. Over a master that can't push, `live()` does nothing and a refresher carries the
load instead.

```admonish note title="The server still never writes"
The live follow only *reads and reconciles*. Nothing in the server process writes to `product`.
That separation keeps the demo honest: whatever you see on screen arrived over the database.
```

### Why not listen for NOTIFY yourself?

You can. A `PgListener` on `product_changed` and a loop is a dozen lines, and an event source the
Vista doesn't model (a webhook, a Kafka topic, a SaaS callback) needs exactly that. The loop hands
each event to the Dio with [`apply_change`](vantage_diorama::Dio::apply_change):

```rust,no_run
# use vantage_diorama::prelude::*;
# use vantage_diorama::{Cache, Loader, LoaderCx};
# use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
#
# #[entity(PostgresType)]
# #[derive(Debug, Clone, Default)]
# pub struct Product {
#     pub name: String,
#     pub price: i64,
#     pub stock: i64,
#     pub created: i64,
# }
#
# impl Product {
#     pub fn table(db: PostgresDB) -> Table<PostgresDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("stock")
#             .with_column_of::<i64>("created")
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
# let db = PostgresDB::connect(&url).await.context("connect postgres")?;
# let master = db.vista_factory().from_table(Product::table(db.clone()))?;
# let lens = Lens::builder()
#     .cache(Cache::memory())
#     .loader(Loader::eager(|cx: LoaderCx| async move {
#         let rows = cx.master().list_values().await?;
#         cx.reconcile(rows).await
#     }))
#     .build()
#     .context("build lens")?;
# let dio = lens.dio(master).build().await?;
// A payload-less ping: re-read the set.
dio.apply_change(ChangeEvent::Invalidated).await?;

// A source that names the row: drop it from the cache, no re-read.
dio.apply_change(ChangeEvent::Deleted { id: "d42".into() }).await?;
# Ok(())
# }
```

`live()` applies changes the same way, so for a Vista that advertises `Subscribe` the hand-written
loop buys nothing and costs two things `live()` already gets right. It subscribes *before* the
eager loader copies the set and applies what arrived once the copy lands, so a write during the
copy is neither lost nor overwritten by it. And a follow that falls behind the stream re-reads the
set instead of dropping changes. Write the loop for sources the Vista can't reach; use `live()`
for the ones it can.

## The till becomes its own process

To drive it, `learn-10` ships a second binary, the **mutator**, run alongside the server:

```console
$ cargo run -p learn-10                 # the reactive server, port 3010
$ cargo run -p learn-10 --bin mutator   # a separate writer process
```

The mutator writes through Vantage's **active-entity** API, with no hand-written SQL.
`list_entities()` hands back drinks that each carry their own id and datasource, so selling one is:

```rust,no_run
# use rand::seq::SliceRandom;
# use vantage_dataset::prelude::ActiveEntitySet;
# use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
#
# #[entity(PostgresType)]
# #[derive(Debug, Clone, Default)]
# pub struct Product {
#     pub name: String,
#     pub price: i64,
#     pub stock: i64,
#     pub created: i64,
# }
#
# impl Product {
#     pub fn table(db: PostgresDB) -> Table<PostgresDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("stock")
#             .with_column_of::<i64>("created")
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
# let db = PostgresDB::connect(&url).await.context("connect postgres")?;
# let table = Product::table(db.clone());
let mut shelf = table.list_entities().await?;
let drink = shelf.choose_mut(&mut rand::thread_rng()).unwrap();
if drink.stock <= 1 {
    drink.delete().await?; // last unit: off the shelf
} else {
    drink.stock -= 1;
    drink.save().await?; // one sale
}
# Ok(())
# }
```

Two independent processes that share nothing but the database and the notify channel: the
strongest proof that the reactive stack tracks the *data*, not an in-process shortcut.

## Removals, and a UI to see them

This app also keys its watch by identity, so a sold-out drink is reported as a removal rather than
a list that shrinks:

```rust,no_run
# use vantage_api_adapters::axum_dio::DioRouter;
# use vantage_diorama::prelude::*;
# use vantage_diorama::{Cache, Loader, LoaderCx};
# use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
#
# #[entity(PostgresType)]
# #[derive(Debug, Clone, Default)]
# pub struct Product {
#     pub name: String,
#     pub price: i64,
#     pub stock: i64,
#     pub created: i64,
# }
#
# impl Product {
#     pub fn table(db: PostgresDB) -> Table<PostgresDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("stock")
#             .with_column_of::<i64>("created")
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
# let db = PostgresDB::connect(&url).await.context("connect postgres")?;
# let master = db
#     .vista_factory()
#     .with_notify(true)
#     .from_table(Product::table(db.clone()))?;
# let lens = Lens::builder()
#     .cache(Cache::memory())
#     .loader(Loader::eager(|cx: LoaderCx| async move {
#         let rows = cx.master().list_values().await?;
#         cx.reconcile(rows).await
#     }))
#     .build()
#     .context("build lens")?;
# let dio = lens.dio(master).live().build().await?;
let api = DioRouter::new(dio.clone())
    .with_column("id", "id")
    .with_column("name", "name")
    .with_column("price", "price")
    .with_column("stock", "stock")
    .key_by("id") // identity-keyed: emits DELETED when a row leaves
    .with_page_size(50)
    .into_router();
# Ok(())
# }
```

With `key_by("id")`, the watch stream gains a third event, `DELETED`, alongside `ADDED` and
`MODIFIED`. The key field has to be one of the `with_column` fields, which is why `id` is exposed
now. A small React page (`frontend/index.html`, served by the same axum app through `ServeDir`)
consumes the stream and animates each drink: arriving with a flash, ticking down as it sells, and
leaving with a "sold out" stamp when a `DELETED` lands. It is plain React over the watch endpoint,
not part of Vantage, and the next chapter reuses the same file unchanged.

Watch it on the wire:

```console
$ curl -N 'localhost:3010/api/products?watch=true'
{"type":"ADDED","object":{"index":0,"id":"d51...","name":"Mojito","price":1000,"stock":11}}
{"type":"ADDED","object":{"index":1,"id":"d59...","name":"Boulevardier","price":1400,"stock":12}}
{"type":"MODIFIED","object":{"index":0,"id":"d51...","name":"Mojito","price":1000,"stock":10}}
{"type":"DELETED","object":{"index":0,"id":"d59...","name":"Boulevardier","price":1400,"stock":1}}
```

Every line is a database write, announced by Postgres, reconciled into the cache, diffed by the
scenery and fanned out, with no timer anywhere in the loop.

```admonish tip title="Coarse push: a ping, then a re-read"
`NOTIFY` carries no row data; the payload is empty. So the Dio learns only *that* something
changed, and responds by re-reading the whole set to reconcile. That is correct and, for a shelf of
a few dozen drinks, instant. But it is a **coarse** signal: one write means one full reconcile. The
next chapter switches to a database whose push carries the changed row itself, so no re-read is
needed.
```

## Checklist

- A `FOR EACH STATEMENT` trigger calls `pg_notify('{table}_changed', '')` on every write.
- `vista_factory().with_notify(true)` declares the trigger, so the Vista advertises `Subscribe`.
- `lens.dio(master).live().build()` follows the channel; each ping refreshes the Dio, and with no
  refresher that re-runs the eager loader's reconcile.
- Without `with_notify(true)` and without a refresher, the cache never sees an outside write.
- `dio.apply_change(ChangeEvent::..)` is the entry for sources the Vista can't subscribe to.
- `.key_by("id")` on the router turns sell-outs into `DELETED` events.

Next: the same app, the same frontend, a different database. SurrealDB's live queries push the
change itself rather than a ping.
