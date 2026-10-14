# Serving & Watching Live Stock

The last chapter left a `Dio` holding a live copy of the `product` table. Now we put it behind
HTTP, using the **same adapter the S3 path uses**. `DioRouter` doesn't know or care that its Dio is
backed by SQL instead of S3: it binds a Dio to a pair of routes, GET and watch, and that is all it
needs.

```rust,no_run
use vantage_api_adapters::axum_dio::DioRouter;
# use std::time::Duration;
#
# use vantage_diorama::prelude::*;
# use vantage_diorama::{Cache, Loader, LoaderCx, Refresher};
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# pub struct Product {
#     pub name: String,
#     pub price: i64,
#     pub stock: i64,
# }
#
# impl Product {
#     pub fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("stock")
#     }
# }
#
# async fn reload(cx: LoaderCx) -> VantageResult<()> {
#     let rows = cx.master().list_values().await?;
#     cx.reconcile(rows).await
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db?mode=rwc").await.context("connect sqlite")?;
# let master = db.vista_factory().from_table(Product::table(db.clone()))?;
# let lens = Lens::builder()
#     .cache(Cache::memory())
#     .loader(Loader::eager(reload))
#     .refresher(Refresher::new(reload).every(Duration::from_secs(1)))
#     .build()
#     .context("build lens")?;
# let dio = lens.dio(master).build().await?;
# let listener = tokio::net::TcpListener::bind("0.0.0.0:3008").await.context("bind :3008")?;

let api = DioRouter::new(dio.clone())
    .with_column("name", "name")
    .with_column("price", "price")
    .with_column("stock", "stock")
    .with_page_size(50)
    .into_router();

let app = axum::Router::new().nest("/api/products", api);
axum::serve(listener, app).await.context("server failed")?;
# Ok(())
# }
```

Each `with_column` maps a JSON key to a record field. The columns double as the demand of every
watch the router opens, which matters on an augmented Dio and is harmless here.

A plain `GET` is the cache, answered at once, never a query against SQLite:

```console
$ curl 'localhost:3008/api/products?offset=0&limit=10'
{"total":5,"offset":0,"limit":10,"items":[
  {"index":0,"name":"Espresso","price":280,"stock":12},
  {"index":1,"name":"Cappuccino","price":340,"stock":8},
  {"index":2,"name":"Cold Brew","price":420,"stock":5},
  {"index":3,"name":"Croissant","price":260,"stock":6},
  {"index":4,"name":"Cheesecake","price":520,"stock":2}]}
```

## A till to make it move

A cache over a table nobody writes is a slow constant. To see the reactive stack work, the data has
to change, so the app runs a **till**: a background task that sells one unit of a random product
every 800ms, removes anything that sells out, and every tenth tick takes a delivery. These are
ordinary SQL writes, sent with `sqlx::query(..).execute(db.pool())` on the same pool the master
reads from (`src/sim.rs`):

```sql
-- sell one unit of a random product that still has stock
UPDATE product SET stock = stock - 1
 WHERE id = (SELECT id FROM product WHERE stock > 0 ORDER BY RANDOM() LIMIT 1);

-- anything that hit zero leaves the shelf
DELETE FROM product WHERE stock <= 0;
```

The server never calls the till and the till never calls the server. They meet only through the
database and the once-a-second reconcile, so **any** writer, in any process, moves the shelf. The
`sqlite3` CLI will do as well, as we'll see.

### Why not have the till write through the Dio?

A Dio is writable, and a write through it shows up in every watch at once instead of on the next
tick. For an app that owns all its writes, that is the better path.

This app doesn't own all its writes. A delivery script, a back-office tool, or a person with
`sqlite3` changes the table behind the server's back, and a Dio that only learned of its own
writes would drift from the database. The till writes straight to SQL on purpose: it stands in
for every writer the server can't see. The price is latency, up to one refresh interval, and the
PostgreSQL chapters remove it.

## The watch

`curl -N` holds the connection open. The page arrives as `ADDED` lines, the current shelf, and
then every sale streams back as a `MODIFIED` line once the reconcile picks it up:

```console
$ curl -N 'localhost:3008/api/products?watch=true'
{"type":"ADDED","object":{"index":0,"name":"Espresso","price":280,"stock":12}}
{"type":"ADDED","object":{"index":1,"name":"Cappuccino","price":340,"stock":8}}
{"type":"ADDED","object":{"index":2,"name":"Cold Brew","price":420,"stock":5}}
{"type":"ADDED","object":{"index":3,"name":"Croissant","price":260,"stock":6}}
{"type":"ADDED","object":{"index":4,"name":"Cheesecake","price":520,"stock":2}}
{"type":"MODIFIED","object":{"index":4,"name":"Cheesecake","price":520,"stock":1}}
{"type":"MODIFIED","object":{"index":2,"name":"Cold Brew","price":420,"stock":4}}
{"type":"MODIFIED","object":{"index":0,"name":"Espresso","price":280,"stock":9}}
```

That is the reactive stack running over SQL, end to end: a SQL write, reconciled into the cache,
one `DatasetChanged` event, a scenery re-derive, a diff against what this connection last saw, one
NDJSON line on the wire. Nothing in that chain is specific to SQL. It is the same machinery the S3
path serves the NOAA bucket with.

```admonish note title="Two events, not three"
`DioRouter` emits only `ADDED` and `MODIFIED` here, never `DELETED`, because it diffs each page
**by position**. A delivery that grows the shelf appears as an `ADDED` at a new index, but a
product that *sells out* shows up as the list shrinking and the rows below it shifting up (each
shifted index reported as `MODIFIED`), not as an explicit removal. A watch client reconstructs
"gone" from the shrinking `total`. A real `DELETED` line is a one-call opt-in: key the watch by
identity with `.key_by("id")`, which the [live-push chapter](./step8-sql-notify.md) turns on.
```

## It really is the database

The strongest proof that this isn't an in-process illusion: change the table from **outside** the
app. With a watch open, in another terminal:

```console
$ sqlite3 learn-8/products.db "UPDATE product SET price=999 WHERE id='p1'"
```

and the open watch stream reflects it within the second:

```text
{"type":"MODIFIED","object":{"index":0,"name":"Espresso","price":999,"stock":11}}
```

No API call and no shared memory: a separate process wrote a row, the reconcile read it, and the
change fanned out to every watcher. The Dio is a mirror of the database, not a copy that happens
to agree with it.

## Checklist

- `DioRouter::new(dio).with_column(..).into_router()` serves GET and watch over any Dio; nest it
  under the resource's path.
- A plain `GET` reads the cache; a `?watch=true` opens a scenery and streams NDJSON.
- Writers outside the server reach watchers through the refresher's reconcile, within one
  interval.
- Without `.key_by(..)`, a watch diffs by position and never sends `DELETED`.

Next: the same app, the same shelf, the same watch, served from PostgreSQL instead of a file and
changed by flipping one compile flag.
