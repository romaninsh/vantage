# Switching to SurrealDB

Postgres was a migration *within* SQL: a different server, the same dialect family. SurrealDB is a
different database: multi-model, schemaless by default, and reached over a WebSocket rather than a
pooled SQL connection. Adopting it tests the claim this path has been making. The goal is to change
**as little as possible**, and then to cash in the one thing SurrealDB does better than a SQL
server: it pushes the *changed row*, not a ping.

The finished crate is [`learn-11`](https://github.com/romaninsh/vantage/tree/main/learn-11). Set
it next to `learn-10` and the diff is small, which is the result we want.

## What changes

Three places name the backend, and nothing else does.

**The entity marker**: swap the backend in the attribute.

```rust
# use vantage_surrealdb::types::AnySurrealType;
# use vantage_types::entity;
#[entity(SurrealType)] // was: #[entity(PostgresType)]
#[derive(Debug, Clone, Default)]
pub struct Product {
    pub name: String,
    pub price: i64,
    pub stock: i64,
    pub created: i64,
}
```

**The table builder**: the same columns, built against `SurrealDB`. SurrealDB's native id is a
`Thing` (`product:⟨key⟩`).

```rust
# use vantage_surrealdb::surrealdb::SurrealDB;
# use vantage_surrealdb::types::AnySurrealType;
# use vantage_table::table::Table;
# use vantage_types::entity;
#
# #[entity(SurrealType)]
# #[derive(Debug, Clone, Default)]
# pub struct Product {
#     pub name: String,
#     pub price: i64,
#     pub stock: i64,
#     pub created: i64,
# }
#
# impl Product {
pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Product> {
    Table::new("product", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<i64>("stock")
        .with_column_of::<i64>("created")
}
# }
```

**Connect and setup**: a DSN instead of a `DATABASE_URL`, and **no trigger**. Where Postgres
needed a `pg_notify` function and an `AFTER … FOR EACH STATEMENT` trigger, SurrealDB emits change
frames on its own. Setup makes sure the table exists:

```rust,no_run
# use surreal_client::SurrealConnection;
# use vantage_core::{Result, error};
# use vantage_surrealdb::surrealdb::SurrealDB;
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<()> {
let dsn = std::env::var("SURREALDB_URL")
    .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bar/v1".to_string());

let client = SurrealConnection::dsn(&dsn)
    .map_err(|e| error!("invalid SURREALDB_URL DSN", details = e.to_string()))?
    .connect()
    .await
    .map_err(|e| error!("connect surrealdb", dsn = &dsn, details = e.to_string()))?;

// The one bit of schema: a table for LIVE SELECT to attach to.
client
    .query("DEFINE TABLE IF NOT EXISTS product SCHEMALESS", None)
    .await
    .map_err(|e| error!("define product table", details = e.to_string()))?;

let db = SurrealDB::new(client);
# Ok(())
# }
```

The till (the mutator) is the same active-entity code as before. Only the id it hands
`new_entity` is a `Thing`:

```rust,no_run
# use vantage_core::{Context, Result};
# use vantage_dataset::prelude::ActiveEntitySet;
# use vantage_surrealdb::surrealdb::SurrealDB;
# use vantage_surrealdb::thing::Thing;
# use vantage_surrealdb::types::AnySurrealType;
# use vantage_table::table::Table;
# use vantage_types::entity;
#
# #[entity(SurrealType)]
# #[derive(Debug, Clone, Default)]
# pub struct Product {
#     pub name: String,
#     pub price: i64,
#     pub stock: i64,
#     pub created: i64,
# }
#
# impl Product {
#     pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#             .with_column_of::<i64>("stock")
#             .with_column_of::<i64>("created")
#     }
# }
#
# async fn deliver(
#     table: &Table<SurrealDB, Product>,
#     key: String,
#     name: String,
#     price: i64,
#     stock: i64,
#     created: i64,
# ) -> Result<()> {
table
    .new_entity(
        Thing::new("product", key), // was: a String id
        Product { name, price, stock, created },
    )
    .save()
    .await?;
# Ok(())
# }
#
# fn main() {}
```

## What doesn't change

The server's reactive core is untouched. The Vista comes from
`SurrealVistaFactory::new(db.clone())` (the factory `db.vista_factory()` returns on a `SurrealDB`)
instead of the Postgres factory, and the Lens and the Dio builder are line for line the Postgres
chapter's, including the call that matters:

```rust,no_run
# use surreal_client::SurrealConnection;
# use vantage_core::{Context, Result, error};
# use vantage_dataset::prelude::*;
# use vantage_diorama::prelude::*;
# use vantage_diorama::{Cache, Loader, LoaderCx};
# use vantage_surrealdb::surrealdb::SurrealDB;
# use vantage_surrealdb::types::AnySurrealType;
# use vantage_surrealdb::vista::SurrealVistaFactory;
# use vantage_table::table::Table;
# use vantage_types::entity;
# use vantage_vista::SortDirection;
#
# #[entity(SurrealType)]
# #[derive(Debug, Clone, Default)]
# pub struct Product {
#     pub name: String,
#     pub price: i64,
#     pub stock: i64,
#     pub created: i64,
# }
#
# impl Product {
#     pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Product> {
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
# async fn main() -> Result<()> {
# let dsn = std::env::var("SURREALDB_URL")
#     .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bar/v1".to_string());
# let client = SurrealConnection::dsn(&dsn)
#     .map_err(|e| error!("invalid SURREALDB_URL DSN", details = e.to_string()))?
#     .connect()
#     .await
#     .map_err(|e| error!("connect surrealdb", dsn = &dsn, details = e.to_string()))?;
# let db = SurrealDB::new(client);
let mut master =
    SurrealVistaFactory::new(db.clone()).from_table(Product::surreal_table(db.clone()))?;
master.set_order(Some(("created", SortDirection::Ascending)))?;

let lens = Lens::builder()
    .cache(Cache::memory())
    .loader(Loader::eager(|cx: LoaderCx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    }))
    .build()
    .context("build lens")?;

let dio = lens.dio(master).live().build().await?; // the same call, now backed by LIVE SELECT
# Ok(())
# }
```

And the **frontend is the same file** as `learn-10`. The React page consumes `?watch=true` and
animates the shelf; it never learns which database is behind the stream. Diff the two
`frontend/index.html` files and you get nothing: the reactive layer, up to and including the UI,
does not depend on the backend.

## The payoff: precise push, no re-read

`live()` looks the same, but underneath it does something better than it could on Postgres.
SurrealDB's `LIVE SELECT` delivers a **typed, per-row** notification for every change: the action
(`CREATE`, `UPDATE`, `DELETE`) *and* the affected record. So the Dio applies each change directly
to its cache, as [`ChangeEvent::Inserted`](vantage_diorama::ChangeEvent::Inserted), `Updated` or
`Deleted` would through `dio.apply_change`. A create inserts one row, an update repaints one row, a
delete removes one row. There is no "something changed, re-read everything" step, because the
change arrived in full.

```console
$ curl -N 'localhost:3011/api/products?watch=true'
{"type":"ADDED","object":{"index":0,"id":{"@@TAGGED@@":[8,["product","d51..."]]},"name":"Mojito","price":1000,"stock":11}}
{"type":"MODIFIED","object":{"index":0,"id":{"@@TAGGED@@":[8,["product","d51..."]]},"name":"Mojito","price":1000,"stock":10}}
{"type":"DELETED","object":{"index":0,"id":{"@@TAGGED@@":[8,["product","d59..."]]},"name":"Boulevardier","price":1400,"stock":1}}
```

The id arrives as a SurrealDB `Thing` rather than a bare string. The frontend's `idOf` helper
normalises either shape to a stable key, which is why the same UI serves both backends.

### Why does SurrealDB need no `with_notify(true)`?

Because there is nothing to forget. Postgres pushes only through a trigger the application
installs, and the driver can't see whether it exists, so the application has to declare it.
SurrealDB's `LIVE SELECT` is part of the server: any table can be followed, with no schema of
yours involved. The Surreal Vista advertises `Subscribe` from the moment you build it, and
`live()` uses it.

The asymmetry is the backend's, not Vantage's. A capability the driver can verify is advertised
by default; one that depends on application setup is opt-in, so a Vista never promises a feed that
may be silent.

```admonish success title="One capability, three freshness models"
The `product` shelf has now run behind three backends. Above the master, what changed was the
Lens losing its refresher and the Dio builder gaining `.live()`:

| Backend | How the Dio learns of a change | Granularity |
|---|---|---|
| SQLite | a `Refresher::new(..).every(..)` timer (it can't push) | poll, whole set |
| PostgreSQL | `LISTEN/NOTIFY`: a payload-less ping, then a re-read | **coarse** push, re-read on each write |
| SurrealDB | `LIVE SELECT`: the action and the row | **fine-grained** push, the row applied directly |

Poll, coarse push, fine push. `live()` picks from what the Vista advertises, and falls back to the
refresher when it advertises nothing. You pick the database your problem has; the freshness you get
is the best that database can offer, and the code that consumes it is the code you already wrote.

Note what each one costs you: SurrealDB is watchable the moment you build the Vista, while the
PostgreSQL row assumes the trigger from the previous chapter *and* the `with_notify(true)` that
declares it. Every other backend Vantage speaks polls today; see [Live Data](../live-data.md) for
the full picture.
```

## Checklist

- The entity marker becomes `#[entity(SurrealType)]`; the columns don't move.
- `SurrealConnection::dsn(..).connect()` replaces `DATABASE_URL`, and `DEFINE TABLE` is the only
  schema.
- New records take a `Thing` id: `Thing::new("product", key)`.
- `SurrealVistaFactory::new(db).from_table(..)` builds the master; the Lens and `.live()` are
  unchanged.
- Each `LIVE SELECT` frame is applied as one row change; nothing is re-read.

From here the reference half of the book takes over: [SurrealDB](../surrealdb.md) for the driver
details, [SQL](../sql.md) for the dialects, [Config-Driven Vistas](../config-driven-vistas.md) for
declaring all of this from YAML, [Live Data](../live-data.md) for what every backend does about
freshness, and [Adding a New Persistence](../new-persistence.md) when your backend isn't one
Vantage already speaks.
