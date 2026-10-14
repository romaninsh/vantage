# Dio & Lens over SQL

This path builds a live, cached, watchable view in front of a SQL database you own. We start from
a Vista over a SQLite `product` table: a runtime handle that can already sort, search, paginate
and write. That is a capable backend, which raises a fair question. If the database already does
all that, what is a caching-and-events layer for?

For the two things a capable backend doesn't hand you on its own. A [Dio](vantage_diorama::Dio)
keeps a **live local copy** of a data segment, so reads are answered from memory instead of
round-trips, and it **announces every change** on an event bus, so a view can watch the data
instead of re-polling it. A slow, read-only API needs a third thing from Diorama: the *injection*
of capabilities the backend lacks, like sorting a listing it cannot sort. A SQL database needs
none of that. Here the live cache and the change stream are the whole story.

```admonish info title="This path assumes the Introduction"
We build on two ideas from the [Introduction](./introduction.md): a
[**Table**](./step2-tables.md), your typed model, and a [**Vista**](./step4-vista.md), the
runtime handle a datasource's factory wraps that model in. If either is new, read the Introduction
first; this path picks up where it ends.
```

We'll build a bar's shelf: a handful of products, each with a `stock` count, behind a cached,
watchable API. A background "till" sells items so the data moves. This chapter builds the cache;
the next serves it as a live watch; the one after moves the whole thing to PostgreSQL without
touching the model.

The full crate is [`learn-8`](https://github.com/romaninsh/vantage/tree/main/learn-8).

## The model

The Introduction's `Product`, with one column added: `stock`, the units on the shelf.

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

Nothing here knows it is about to be cached, watched, or served from Postgres. The entity is
written once, and the layers above it decide what to do with it.

## The master

The master is the `product` table as a Vista, built the way the [Introduction](./step4-vista.md)
built one: hand the table to its datasource's Vista factory.

```rust,no_run
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
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
const DB_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/products.db");

let url = format!("sqlite:{DB_PATH}?mode=rwc");
let db = SqliteDB::connect(&url).await.context("connect sqlite")?;
let master = db.vista_factory().from_table(Product::table(db.clone()))?;
# Ok(())
# }
```

The database is a file beside the crate, and `mode=rwc` creates it on first run. A file rather
than `sqlite::memory:` is deliberate: the next chapter edits it from another terminal with the
`sqlite3` CLI and watches the change arrive.

Ask this Vista what it can do and its [capabilities](vantage_vista::Capability) cover everything a
listing needs: `Order`, `Search`, `FilterOps` and `Window`, plus `Insert`, `Update` and `Delete`.
Contrast that with the S3 path, where the Vista admits it cannot sort or search. There is no gap to
fill here, so this path injects no capabilities. What we still want is the cache.

## The lens: eager and reactive

The S3 path's loader walks the bucket a page at a time, resuming from a cursor, and writes each
page into a redb file. A SQL table needs none of that: it hands back the whole set in one call. So
this path uses an eager loader that lists the table once, and a refresher that lists it again on a
timer.

```rust,no_run
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
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:products.db?mode=rwc").await.context("connect sqlite")?;
# let master = db.vista_factory().from_table(Product::table(db.clone()))?;
let lens = Lens::builder()
    .cache(Cache::memory())
    .loader(Loader::eager(reload))
    .refresher(Refresher::new(reload).every(Duration::from_secs(1)))
    .build()
    .context("build lens")?;
let dio = lens.dio(master).build().await?;
# Ok(())
# }

/// Make the cache hold exactly the master's rows.
async fn reload(cx: LoaderCx) -> VantageResult<()> {
    let rows = cx.master().list_values().await?;
    cx.reconcile(rows).await
}
```

Three settings carry the behaviour:

- **[`Cache::memory()`](vantage_diorama::Cache::memory)**: the cache is ephemeral, and a restart
  re-reads the table. The S3 path persists its cache with `Cache::redb("cache.redb")` because
  re-listing a remote bucket is expensive. Re-reading a local table is not.
- **[`Loader::eager(reload)`](vantage_diorama::Loader::eager)**: before the server answers a
  request, `list_values()` pulls the whole table into the cache. `lens.dio(master).build()` waits
  for it by default, so the first read is already warm.
- **[`Refresher::new(reload).every(..)`](vantage_diorama::Refresher)**: once a second, `reload`
  runs again. This is what makes the copy *live*: a sale the till commits, or an edit typed into
  `sqlite3`, lands on the next tick.

The loader and the refresher share one function because both answer the same question: what
should the cache hold? [`LoaderCx::reconcile`](vantage_diorama::LoaderCx::reconcile) takes the
master's answer and makes the cache hold exactly that set. It writes the rows that differ, deletes
the cached rows the master no longer returns (a product that sold out), leaves unchanged rows
alone, and publishes one `DatasetChanged` for watchers.

### Why reconcile instead of clearing and reloading?

The obvious refresh clears the cache and inserts the fresh rows. It has two problems.

The first is the gap. Between the clear and the insert the cache is empty, and a reader that lands
in that gap sees an empty shelf. Reconcile never empties the cache: each row is kept, rewritten,
or removed.

The second is writes in flight. A Dio takes writes optimistically: the cache shows the new value at
once, and the master catches up. A refresh that listed the master just before that write landed
would put the old value back. Reconcile skips any row with a write in flight, and any row whose
write settled after this refresh began listing, so a refresh can't undo a write it raced.

The cost is a comparison per row on every tick. For a shelf of products that is noise. For a table
of a million rows, a one-second full reconcile is the wrong tool, and the PostgreSQL chapters
replace the timer with push.

## Reads come from the cache

From here on, reads don't touch SQLite. Everything a consumer asks of the Dio, a listing, a page
or a single record, is answered from the in-memory cache the eager loader warmed. The database is
consulted by the once-a-second reconcile and by nothing else. The next chapter puts a `DioRouter`
over this Dio, and a plain `GET` returns the shelf without a query reaching the table.

The backend stays the source of truth, but the handle you *read* is a local, live mirror of it.

```admonish note title="What a capable backend changes, and what it doesn't"
The lens changed: a one-call list instead of a cursor walk, memory instead of redb, a refresher
where S3 had none, and no augmentation because a SQL row already carries its columns. What *didn't*
change is everything above `lens.dio(master).build()`: the `Dio`, its event bus, and the sceneries
and watch server the next chapter opens over it are the same code the S3 path uses. The reactive
stack doesn't know which backend it sits on.
```

## Checklist

- The model is a plain `Table` with an `#[entity]` struct; nothing in it mentions caching.
- The master is `db.vista_factory().from_table(..)`, and a SQL Vista needs no capability injected.
- One `reload` function lists the master and hands the rows to `cx.reconcile`; the loader and the
  refresher both call it.
- `Cache::memory()` because re-reading a local table is cheap; `Cache::redb(..)` when it isn't.
- `Refresher::new(..).every(..)` is what brings outside writes in, until the PostgreSQL chapters
  switch to push.

Next: serve this Dio as a Kubernetes-style watch, and turn on the till so the shelf moves while
you look at it.
