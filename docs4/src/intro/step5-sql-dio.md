# Dio & Lens over SQL

This path builds a live, cached, watchable view in front of a SQL database you own. We start from
a Vista over a SQLite `product` table — a runtime handle that can already sort, search, paginate,
and write. It is a fully capable backend, which raises a fair question: if the database already
does all that, what is a caching-and-events layer even for?

For the two things a capable backend doesn't hand you on its own. A `Dio` keeps a **live local
copy** of a data segment — reads answered from memory, not round-trips — and it **announces every
change** on an event bus, so a view can watch the data instead of re-polling it. (A slow, read-only
API needs a third thing from Diorama — the *injection* of capabilities the backend lacks, like
sorting a listing it cannot sort. A SQL database needs none of that; here the live cache and the
change stream are the whole story.)

```admonish info title="This path assumes the Introduction"
We build straight on two ideas from the [Introduction](./introduction.md): a
[**Table**](./step2-tables.md) — your typed model — and a [**Vista**](./step4-vista.md), the
runtime handle a datasource's factory wraps that model in. If either is new, read the Introduction
first; this path picks up exactly where it ends.
```

We'll build a bar's shelf: a handful of products, each with a `stock` count, behind a cached,
watchable API. A background "till" will sell items so the data actually moves. This chapter builds
the cache; the next serves it as a live watch; the last moves the whole thing to PostgreSQL
without touching a line of the model.

The full crate is [`learn-8`](https://github.com/romaninsh/vantage/tree/main/learn-8).

## The model

The Introduction's `Product`, with one column added — `stock`, the units on the shelf:

```rust
// src/product.rs
use vantage_sql::prelude::*;
use vantage_types::prelude::*;

#[entity(SqliteType)]
#[derive(Debug, Clone, Default)]
pub struct Product {
    pub name: String,
    pub price: i64,
    pub stock: i64,
}

impl Product {
    pub fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("price")
            .with_column_of::<i64>("stock")
    }
}
```

Nothing here knows it is about to be cached, watched, or served from Postgres. That is the whole
bet of the four-layer model — the entity is written once and the layers above it decide what to do
with it.

## The master

The master is the `product` table as a Vista — built exactly the way the
[Introduction](./step4-vista.md) built one: hand the table to its datasource's Vista factory.

```rust
let db = SqliteDB::connect(&url).await?;
let master = db.vista_factory().from_table(Product::table(db.clone()))?;
```

Ask this Vista what it can do and it answers *yes* to everything — `can_order`, `can_fetch_window`,
`can_insert`, `can_update`, `can_delete`. Contrast that with the S3 path, where the Vista admits it
cannot sort or search. There is no gap to fill here, so there is no capability injection in this
path. What we still want is the cache.

## The lens: eager and reactive

A slow, paginated backend like S3 is loaded a page at a time, resuming from a cursor. A SQL table
needs none of that ceremony — it hands back the whole set in one call — so this path uses the
**eager** shape of a Lens: load everything on start, then reconcile on a timer.

```rust
let lens = Arc::new(
    Lens::new()
        .cache_in_memory()
        .on_start(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.master().list_values().await?;
                dio.cache().insert_values(rows).await?;
                Ok(())
            }
        })
        .on_refresh(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.master().list_values().await?;
                dio.cache().clear().await?;
                dio.cache().insert_values(rows).await?;
                Ok(())
            }
        })
        .refresh_every(Duration::from_secs(1))
        .build()?,
);
let dio = lens.make_dio(master).await?;
```

Three builder calls carry the whole behaviour:

- **`cache_in_memory()`** — the cache is ephemeral; a restart re-reads the table. (The S3 path
  persists its cache with `cache_at("cache.redb")` because re-listing a remote bucket is expensive.
  Re-reading a local table is not.)
- **`on_start`** — before the server answers a single request, `list_values()` pulls the entire
  table into the cache. `make_dio` blocks on this, so the first read is already warm.
- **`on_refresh` + `refresh_every`** — once a second, the cache is rebuilt from the master. This is
  what makes the copy *live*: a sale the till commits, or an edit you type into `sqlite3`, is
  reconciled on the next tick, and a `DatasetChanged` event goes out to every watcher. The
  clear-then-reload is deliberate — a product that has sold out is simply absent from the master's
  answer, so clearing first drops it cleanly.

## Reads come from the cache

From here on, reads never touch SQLite. `make_dio` returned a `Dio`, and everything a consumer asks
of it — a listing, a page, a single record — is answered from the in-memory cache the `on_start`
warmed. The database is consulted only by the once-a-second reconcile, never by a reader. The next
chapter puts a `DioRouter` over this Dio and you'll see a plain `GET` return the shelf instantly,
without a query reaching the table.

That is the inversion the whole guide has been climbing toward: the backend is the source of truth,
but the handle you *read* is a local, live mirror of it.

```admonish note title="What a capable backend changes — and what it doesn't"
The lens shape changed — eager full-load instead of paged, in-memory instead of redb, no
augmentation because a SQL row already carries its columns. What *didn't* change is everything
above `make_dio`: the `Dio`, its event bus, and the sceneries and watch server the next chapter
opens over it are byte-for-byte the same code the S3 path uses. The reactive stack never learns
which backend it sits on.
```

Next: serve this Dio as a Kubernetes-style watch, and turn on the till so the shelf moves while
you look at it.
