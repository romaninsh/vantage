# Serving & Watching Live Stock

Last chapter left a `Dio` holding a live copy of the `product` table. Now we put it behind HTTP —
using the **same adapter the S3 path uses**. `DioRouter` neither knows nor cares that its Dio is backed
by SQL instead of S3; it binds a Dio to a pair of routes, GET and watch, and that is all it needs.

```rust
use vantage_api_adapters::axum_dio::DioRouter;

let api = DioRouter::new(dio.clone())
    .with_column("name", "name")
    .with_column("price", "price")
    .with_column("stock", "stock")
    .with_page_size(50)
    .into_router();

let app = axum::Router::new().nest("/api/products", api);
axum::serve(listener, app).await?;
```

A plain `GET` is the cache, answered instantly — never a query against SQLite:

```text
$ curl 'localhost:3008/api/products?offset=0&limit=10'
{"total":5,"offset":0,"limit":10,"items":[
  {"index":0,"name":"Espresso","price":280,"stock":12},
  {"index":1,"name":"Cappuccino","price":340,"stock":8},
  {"index":2,"name":"Cold Brew","price":420,"stock":5},
  {"index":3,"name":"Croissant","price":260,"stock":6},
  {"index":4,"name":"Cheesecake","price":520,"stock":2}]}
```

## A till to make it move

A cache over a table nobody writes is just a slow constant. To see the reactive stack work, the
data has to change — so the app runs a **till**: a background task that sells one unit of a random
product every 800ms, removes anything that sells out, and every so often takes a delivery. These
are ordinary SQL writes, on the same connection the master reads from:

```rust
// src/sim.rs — the interesting line
sqlx::query(
    "UPDATE product SET stock = stock - 1
     WHERE id = (SELECT id FROM product WHERE stock > 0 ORDER BY RANDOM() LIMIT 1)",
)
.execute(db.pool())
.await?;
```

The server itself never calls the till and the till never calls the server. They meet only through
the database and the once-a-second reconcile — which is precisely the point: **any** writer, in any
process, moves the shelf. The `sqlite3` CLI will do just as well, as we'll see.

## The watch

`curl -N` holds the connection open. The page arrives as `ADDED` lines — the current shelf — and
then every sale streams back as a `MODIFIED` line the moment the reconcile picks it up:

```text
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

That is the reactive stack running over SQL, end to end: a SQL write, reconciled into the cache, a
`DatasetChanged` event, a scenery re-derive, a diff against what this connection last saw, one
NDJSON line on the wire. Nothing in that chain is specific to SQL — it is the same machinery the
S3 path serves the NOAA bucket with.

```admonish note title="Two events, not three"
`DioRouter` emits only `ADDED` and `MODIFIED`, never `DELETED` — it diffs each page **by
position**. So a delivery that grows the shelf appears as an `ADDED` at a new index, but a product
that *sells out* shows up as the list shrinking and the rows below it shifting up (each shifted
index reported as `MODIFIED`), not as an explicit removal. A watch client reconstructs "gone" from
the shrinking `total`. Translating removals into a `DELETED` line would mean extending the adapter —
a fair exercise, and a reminder that the adapter is ordinary code, not framework magic.
```

## It really is the database

The strongest proof that this isn't an in-process illusion: change the table from **outside** the
app entirely. With a watch open, in another terminal:

```text
$ sqlite3 learn-8/products.db "UPDATE product SET price=999 WHERE id='p1'"
```

and the open watch stream reflects it within the second:

```text
{"type":"MODIFIED","object":{"index":0,"name":"Espresso","price":999,"stock":11}}
```

No API call, no shared memory — a separate process wrote a row, the reconcile read it, and the
change fanned out to every watcher. The Dio is a mirror of the database, not a copy that happens to
agree with it.

Next: the same app, the same shelf, the same watch — served from PostgreSQL instead of a file,
changed by flipping one compile flag.
